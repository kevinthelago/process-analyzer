use std::sync::Arc;

use arrow::{
    array::{Array, Int64Array, StringArray, UInt32Array, UInt64Array},
    record_batch::RecordBatch,
};
use query::{
    queries::histogram::LatencySource,
    QueryEngine, Selection, TimeRange, TraceStore,
    cpu_samples_schema, disk_io_schema, file_io_schema, frames_schema,
    memory_schema, processes_schema, scheduling_schema, stacks_schema,
    threads_schema,
};
use tokio_util::sync::CancellationToken;

// ── Mock store ────────────────────────────────────────────────────────────────

struct MockStore {
    cpu_samples: Vec<RecordBatch>,
    stacks:      Vec<RecordBatch>,
    frames:      Vec<RecordBatch>,
    scheduling:  Vec<RecordBatch>,
    disk_io:     Vec<RecordBatch>,
    file_io:     Vec<RecordBatch>,
    memory:      Vec<RecordBatch>,
    processes:   Vec<RecordBatch>,
    threads:     Vec<RecordBatch>,
    duration_ns: u64,
}

impl MockStore {
    fn with_data() -> Self {
        // 5 CPU samples: process_id 1 gets samples 1-3 (3 samples),
        //                process_id 2 gets samples 4-5 (2 samples).
        // stack_id 1-5, each with a 2-frame stack (depth 0 = leaf, depth 1 = caller).
        let cpu = RecordBatch::try_new(
            cpu_samples_schema(),
            vec![
                Arc::new(Int64Array::from(vec![100i64, 200, 300, 400, 500])),
                Arc::new(UInt32Array::from(vec![1u32, 1, 1, 2, 2])),
                Arc::new(UInt32Array::from(vec![10u32, 10, 10, 20, 20])),
                Arc::new(UInt32Array::from(vec![0u32; 5])),
                Arc::new(UInt64Array::from(vec![1u64; 5])),
                Arc::new(UInt64Array::from(vec![Some(1u64), Some(2), Some(3), Some(4), Some(5)])),
            ],
        ).unwrap();

        // Stacks: each stack_id has depth 0 (leaf) and depth 1 (caller).
        // stack_id 1,2,3 → leaf = "inner", caller = "process"
        // stack_id 4,5   → leaf = "worker_fn", caller = "inner"
        let stacks = RecordBatch::try_new(
            stacks_schema(),
            vec![
                // stack_id
                Arc::new(UInt64Array::from(vec![1u64,1,2,2,3,3,4,4,5,5])),
                // depth
                Arc::new(UInt32Array::from(vec![0u32,1,0,1,0,1,0,1,0,1])),
                // frame_id: depth0→frame A,B,A,B,A,B,C,B,C,D
                Arc::new(UInt64Array::from(vec![1u64,2,1,2,1,2,3,2,3,4])),
            ],
        ).unwrap();

        // Frames: id 1=inner, 2=process, 3=worker_fn, 4=main
        let frames = RecordBatch::try_new(
            frames_schema(),
            vec![
                Arc::new(UInt64Array::from(vec![1u64,2,3,4])),
                Arc::new(UInt64Array::from(vec![0u64,0,0,0])),
                Arc::new(StringArray::from(vec![Some("inner"), Some("process"), Some("worker_fn"), Some("main")])),
                Arc::new(StringArray::from(vec![Some("app"), Some("app"), Some("app"), Some("app")])),
                Arc::new(StringArray::from(vec![None::<&str>;4])),
                Arc::new(UInt32Array::from(vec![None::<u32>;4])),
            ],
        ).unwrap();

        // Scheduling: 1 event with 12 ms duration (above 5 ms threshold)
        let sched = RecordBatch::try_new(
            scheduling_schema(),
            vec![
                Arc::new(Int64Array::from(vec![150i64])),
                Arc::new(UInt32Array::from(vec![1u32])),
                Arc::new(UInt32Array::from(vec![10u32])),
                Arc::new(UInt32Array::from(vec![0u32])),
                Arc::new(arrow::array::UInt8Array::from(vec![0u8])),
                Arc::new(arrow::array::UInt8Array::from(vec![None::<u8>])),
                Arc::new(UInt32Array::from(vec![None::<u32>])),
                Arc::new(UInt32Array::from(vec![None::<u32>])),
                Arc::new(UInt64Array::from(vec![Some(12_000_000u64)])),
            ],
        ).unwrap();

        // Disk I/O: process 1 reads 4096 bytes, process 2 writes 2048 bytes
        let disk = RecordBatch::try_new(
            disk_io_schema(),
            vec![
                Arc::new(Int64Array::from(vec![200i64, 350])),
                Arc::new(UInt32Array::from(vec![1u32, 2])),
                Arc::new(UInt32Array::from(vec![10u32, 20])),
                Arc::new(arrow::array::UInt8Array::from(vec![0u8, 1])),
                Arc::new(UInt32Array::from(vec![8u32, 8])),
                Arc::new(UInt32Array::from(vec![1u32, 1])),
                Arc::new(UInt64Array::from(vec![0u64, 0])),
                Arc::new(UInt64Array::from(vec![4096u64, 2048])),
                Arc::new(UInt64Array::from(vec![Some(5_000_000u64), Some(3_000_000)])),
            ],
        ).unwrap();

        // File I/O: process 1 reads 8192 bytes
        let file = RecordBatch::try_new(
            file_io_schema(),
            vec![
                Arc::new(Int64Array::from(vec![250i64])),
                Arc::new(UInt32Array::from(vec![1u32])),
                Arc::new(UInt32Array::from(vec![10u32])),
                Arc::new(arrow::array::UInt8Array::from(vec![0u8])),
                Arc::new(arrow::array::Int32Array::from(vec![Some(3i32)])),
                Arc::new(UInt64Array::from(vec![Some(12345u64)])),
                Arc::new(Int64Array::from(vec![Some(0i64)])),
                Arc::new(UInt64Array::from(vec![Some(8192u64)])),
                Arc::new(UInt64Array::from(vec![Some(2_000_000u64)])),
                Arc::new(Int64Array::from(vec![Some(8192i64)])),
            ],
        ).unwrap();

        let mem = RecordBatch::new_empty(memory_schema());

        let procs = RecordBatch::try_new(
            processes_schema(),
            vec![
                Arc::new(UInt32Array::from(vec![1u32, 2])),
                Arc::new(UInt32Array::from(vec![None::<u32>, Some(1)])),
                Arc::new(StringArray::from(vec!["main-app", "worker"])),
                Arc::new(StringArray::from(vec![None::<&str>, None])),
                Arc::new(Int64Array::from(vec![0i64, 50])),
                Arc::new(Int64Array::from(vec![None::<i64>, None])),
                Arc::new(arrow::array::Int32Array::from(vec![None::<i32>, None])),
            ],
        ).unwrap();

        let threads = RecordBatch::new_empty(threads_schema());

        Self {
            cpu_samples: vec![cpu],
            stacks: vec![stacks],
            frames: vec![frames],
            scheduling: vec![sched],
            disk_io: vec![disk],
            file_io: vec![file],
            memory: vec![mem],
            processes: vec![procs],
            threads: vec![threads],
            duration_ns: 1_000_000_000,
        }
    }
}

impl TraceStore for MockStore {
    fn cpu_samples(&self) -> &[RecordBatch] { &self.cpu_samples }
    fn stacks(&self)      -> &[RecordBatch] { &self.stacks }
    fn frames(&self)      -> &[RecordBatch] { &self.frames }
    fn scheduling(&self)  -> &[RecordBatch] { &self.scheduling }
    fn disk_io(&self)     -> &[RecordBatch] { &self.disk_io }
    fn file_io(&self)     -> &[RecordBatch] { &self.file_io }
    fn memory(&self)      -> &[RecordBatch] { &self.memory }
    fn processes(&self)   -> &[RecordBatch] { &self.processes }
    fn threads(&self)     -> &[RecordBatch] { &self.threads }
    fn duration_ns(&self) -> u64            { self.duration_ns }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_top_n_by_cpu_returns_sorted_results() {
    let store = MockStore::with_data();
    let engine = QueryEngine::new();
    let token = CancellationToken::new();

    let batch = engine
        .top_n_by_cpu(&store, &Selection::default(), 10, token)
        .await
        .expect("top_n_by_cpu failed");

    assert!(batch.num_rows() > 0, "should return at least one row");

    // DataFusion COUNT(*) returns Int64.
    let counts = batch
        .column_by_name("sample_count")
        .expect("missing sample_count")
        .as_any()
        .downcast_ref::<Int64Array>()
        .expect("sample_count should be Int64");

    for i in 1..counts.len() {
        assert!(
            counts.value(i - 1) >= counts.value(i),
            "results not sorted descending at index {i}"
        );
    }
}

#[tokio::test]
async fn test_top_n_by_cpu_with_pid_filter() {
    let store = MockStore::with_data();
    let engine = QueryEngine::new();
    let token = CancellationToken::new();
    let sel = Selection {
        pids: Some(vec![1]),
        ..Default::default()
    };

    let batch = engine
        .top_n_by_cpu(&store, &sel, 10, token)
        .await
        .expect("top_n_by_cpu with pid filter failed");

    let pid_col = batch
        .column_by_name("process_id")
        .expect("missing process_id")
        .as_any()
        .downcast_ref::<UInt32Array>()
        .expect("process_id should be UInt32");

    for i in 0..pid_col.len() {
        assert_eq!(pid_col.value(i), 1, "expected only process_id=1 in result");
    }
}

#[tokio::test]
async fn test_top_n_functions_self_time() {
    let store = MockStore::with_data();
    let engine = QueryEngine::new();
    let token = CancellationToken::new();

    let batch = engine
        .top_n_functions(&store, &Selection::default(), 20, token)
        .await
        .expect("top_n_functions failed");

    assert!(batch.num_rows() > 0);
    // "inner" is the depth-0 frame for stacks 1,2,3 (3 self samples).
    let names = batch
        .column_by_name("symbol_name")
        .unwrap()
        .as_any()
        .downcast_ref::<StringArray>()
        .unwrap();
    let all_names: Vec<&str> = (0..names.len()).map(|i| names.value(i)).collect();
    assert!(
        all_names.contains(&"inner"),
        "expected 'inner' in top functions; got {all_names:?}"
    );
}

#[tokio::test]
async fn test_flame_graph_aggregation() {
    let store = MockStore::with_data();
    let engine = QueryEngine::new();
    let token = CancellationToken::new();

    let batch = engine
        .flame_graph_aggregation(&store, &Selection::default(), token)
        .await
        .expect("flame_graph_aggregation failed");

    assert!(batch.num_rows() > 0);
    assert!(batch.column_by_name("symbol_name").is_some());
    assert!(batch.column_by_name("self_samples").is_some());
    assert!(batch.column_by_name("total_samples").is_some());
}

#[tokio::test]
async fn test_flame_graph_edges() {
    let store = MockStore::with_data();
    let engine = QueryEngine::new();
    let token = CancellationToken::new();

    let batch = engine
        .flame_graph_edges(&store, &Selection::default(), token)
        .await
        .expect("flame_graph_edges failed");

    assert!(batch.num_rows() > 0);
    // The stacks have depth0=inner (leaf), depth1=process (caller).
    // So the edge should be process→inner (parent is at higher depth = caller).
    let parent = batch
        .column_by_name("parent_fn")
        .unwrap()
        .as_any()
        .downcast_ref::<StringArray>()
        .unwrap();
    let child = batch
        .column_by_name("child_fn")
        .unwrap()
        .as_any()
        .downcast_ref::<StringArray>()
        .unwrap();
    let pairs: Vec<(&str, &str)> = (0..parent.len())
        .map(|i| (parent.value(i), child.value(i)))
        .collect();
    assert!(
        pairs.contains(&("process", "inner")),
        "expected process→inner edge (depth1→depth0); got {pairs:?}"
    );
}

#[tokio::test]
async fn test_time_histogram_bucket_count() {
    let store = MockStore::with_data();
    let engine = QueryEngine::new();
    let token = CancellationToken::new();

    let batch = engine
        .time_histogram(&store, &Selection::default(), 10, token)
        .await
        .expect("time_histogram failed");

    assert!(batch.num_rows() <= 10, "should not exceed requested bucket count");
    assert!(batch.num_rows() > 0, "should have at least one non-empty bucket");
}

#[tokio::test]
async fn test_time_histogram_with_time_selection() {
    let store = MockStore::with_data();
    let engine = QueryEngine::new();
    let token = CancellationToken::new();
    let sel = Selection {
        time_range: Some(TimeRange::new(100, 300)),
        ..Default::default()
    };

    let batch = engine
        .time_histogram(&store, &sel, 5, token)
        .await
        .expect("time_histogram with selection failed");

    // cpu_samples at 100, 200, 300 fall in [100, 300] — 3 cpu events.
    // disk_io at 200 and file_io at 250 also fall in range.
    let cpu_samples = batch
        .column_by_name("cpu_samples")
        .unwrap()
        .as_any()
        .downcast_ref::<Int64Array>()
        .unwrap();
    let total_cpu: i64 = (0..cpu_samples.len()).map(|i| cpu_samples.value(i)).sum();
    assert_eq!(total_cpu, 3i64, "expected 3 cpu_sample events in [100, 300]");
}

#[tokio::test]
async fn test_latency_distribution_scheduling() {
    let store = MockStore::with_data();
    let engine = QueryEngine::new();
    let token = CancellationToken::new();

    let batch = engine
        .latency_distribution(&store, &Selection::default(), LatencySource::Scheduling, 5, token)
        .await
        .expect("latency_distribution failed");

    assert!(batch.num_rows() > 0);
    assert!(batch.column_by_name("lower_ns").is_some());
    assert!(batch.column_by_name("upper_ns").is_some());
    assert!(batch.column_by_name("count").is_some());
}

#[tokio::test]
async fn test_cancellation_is_honoured() {
    let store = MockStore::with_data();
    let engine = QueryEngine::new();
    let token = CancellationToken::new();
    token.cancel();

    let result = engine
        .top_n_by_cpu(&store, &Selection::default(), 10, token)
        .await;

    assert!(
        matches!(result, Err(query::QueryError::Cancelled)),
        "expected Cancelled error, got {result:?}"
    );
}

#[tokio::test]
async fn test_top_n_by_io() {
    let store = MockStore::with_data();
    let engine = QueryEngine::new();
    let token = CancellationToken::new();

    let batch = engine
        .top_n_by_io(&store, &Selection::default(), 10, token)
        .await
        .expect("top_n_by_io failed");

    assert!(batch.num_rows() > 0);
    let io_bytes = batch
        .column_by_name("io_bytes")
        .unwrap()
        .as_any()
        .downcast_ref::<Int64Array>()
        .unwrap();
    // process 1: disk 4096 + file 8192 = 12288; process 2: disk 2048
    assert_eq!(io_bytes.value(0), 12288, "process 1 should have 12288 io bytes total");
}
