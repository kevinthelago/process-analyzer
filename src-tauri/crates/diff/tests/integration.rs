use std::sync::Arc;

use arrow::{
    array::{Int32Array, Int64Array, UInt8Array, UInt32Array, UInt64Array, StringArray},
    record_batch::RecordBatch,
};
use diff::{ChangeKind, DiffEngine};
use query::{
    Selection, TraceStore,
    cpu_samples_schema, stacks_schema, frames_schema,
    disk_io_schema, processes_schema,
};
use tokio_util::sync::CancellationToken;

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Returns (cpu_samples, stacks, frames) for `count` samples all attributed to
/// `symbol_name` at depth 0 (leaf), all from `pid`.
fn make_fn_data(pid: u32, symbol_name: &str, count: usize) -> (RecordBatch, RecordBatch, RecordBatch) {
    let cpu_samples = RecordBatch::try_new(
        cpu_samples_schema(),
        vec![
            Arc::new(Int64Array::from((0..count as i64).map(|i| i * 100).collect::<Vec<_>>())),
            Arc::new(UInt32Array::from(vec![pid; count])),
            Arc::new(UInt32Array::from(vec![10u32; count])),
            Arc::new(UInt32Array::from(vec![0u32; count])),
            Arc::new(UInt64Array::from(vec![1u64; count])),
            Arc::new(UInt64Array::from(vec![Some(1u64); count])),
        ],
    )
    .unwrap();

    let stacks = RecordBatch::try_new(
        stacks_schema(),
        vec![
            Arc::new(UInt64Array::from(vec![1u64])),
            Arc::new(UInt32Array::from(vec![0u32])),
            Arc::new(UInt64Array::from(vec![1u64])),
        ],
    )
    .unwrap();

    let frames = RecordBatch::try_new(
        frames_schema(),
        vec![
            Arc::new(UInt64Array::from(vec![1u64])),
            Arc::new(UInt64Array::from(vec![0u64])),
            Arc::new(StringArray::from(vec![Some(symbol_name)])),
            Arc::new(StringArray::from(vec![Some("app")])),
            Arc::new(StringArray::from(vec![None::<&str>])),
            Arc::new(UInt32Array::from(vec![None::<u32>])),
        ],
    )
    .unwrap();

    (cpu_samples, stacks, frames)
}

fn make_disk_io(pid: u32, size_bytes: u64, duration_ns: u64) -> RecordBatch {
    RecordBatch::try_new(
        disk_io_schema(),
        vec![
            Arc::new(Int64Array::from(vec![500_000i64])),
            Arc::new(UInt32Array::from(vec![pid])),
            Arc::new(UInt32Array::from(vec![10u32])),
            Arc::new(UInt8Array::from(vec![0u8])),
            Arc::new(UInt32Array::from(vec![8u32])),
            Arc::new(UInt32Array::from(vec![0u32])),
            Arc::new(UInt64Array::from(vec![0u64])),
            Arc::new(UInt64Array::from(vec![size_bytes])),
            Arc::new(UInt64Array::from(vec![Some(duration_ns)])),
        ],
    )
    .unwrap()
}

fn make_processes(pid: u32, name: &str) -> RecordBatch {
    RecordBatch::try_new(
        processes_schema(),
        vec![
            Arc::new(UInt32Array::from(vec![pid])),
            Arc::new(UInt32Array::from(vec![None::<u32>])),
            Arc::new(StringArray::from(vec![name])),
            Arc::new(StringArray::from(vec![None::<&str>])),
            Arc::new(Int64Array::from(vec![0i64])),
            Arc::new(Int64Array::from(vec![None::<i64>])),
            Arc::new(Int32Array::from(vec![None::<i32>])),
        ],
    )
    .unwrap()
}

// ── Mock store ────────────────────────────────────────────────────────────────

struct FixedStore {
    cpu_samples: Vec<RecordBatch>,
    stacks:      Vec<RecordBatch>,
    frames:      Vec<RecordBatch>,
    disk_io:     Vec<RecordBatch>,
    processes:   Vec<RecordBatch>,
    duration_ns: u64,
}

impl FixedStore {
    fn new(
        cpu_samples: Vec<RecordBatch>,
        stacks:      Vec<RecordBatch>,
        frames:      Vec<RecordBatch>,
        disk_io:     Vec<RecordBatch>,
        processes:   Vec<RecordBatch>,
        duration_ns: u64,
    ) -> Self {
        Self { cpu_samples, stacks, frames, disk_io, processes, duration_ns }
    }

    fn empty_fn_tables() -> (Vec<RecordBatch>, Vec<RecordBatch>, Vec<RecordBatch>) {
        (
            vec![RecordBatch::new_empty(cpu_samples_schema())],
            vec![RecordBatch::new_empty(stacks_schema())],
            vec![RecordBatch::new_empty(frames_schema())],
        )
    }
}

impl TraceStore for FixedStore {
    fn cpu_samples(&self) -> &[RecordBatch] { &self.cpu_samples }
    fn stacks(&self)       -> &[RecordBatch] { &self.stacks }
    fn frames(&self)       -> &[RecordBatch] { &self.frames }
    fn scheduling(&self)   -> &[RecordBatch] { &[] }
    fn disk_io(&self)      -> &[RecordBatch] { &self.disk_io }
    fn file_io(&self)      -> &[RecordBatch] { &[] }
    fn memory(&self)       -> &[RecordBatch] { &[] }
    fn processes(&self)    -> &[RecordBatch] { &self.processes }
    fn threads(&self)      -> &[RecordBatch] { &[] }
    fn duration_ns(&self)  -> u64            { self.duration_ns }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_diff_detects_function_regression() {
    // Trace A: slow_fn has 2 samples. Trace B: slow_fn has 8 samples — regression.
    let (cs_a, st_a, fr_a) = make_fn_data(1, "slow_fn", 2);
    let (cs_b, st_b, fr_b) = make_fn_data(1, "slow_fn", 8);

    let store_a = FixedStore::new(
        vec![cs_a], vec![st_a], vec![fr_a],
        vec![RecordBatch::new_empty(disk_io_schema())],
        vec![make_processes(1, "app")],
        1_000_000,
    );
    let store_b = FixedStore::new(
        vec![cs_b], vec![st_b], vec![fr_b],
        vec![RecordBatch::new_empty(disk_io_schema())],
        vec![make_processes(1, "app")],
        1_000_000,
    );

    let result = DiffEngine::new()
        .diff(&store_a, &store_b, &Selection::default(), CancellationToken::new())
        .await
        .unwrap();

    let delta = result
        .function_deltas
        .iter()
        .find(|d| d.function_name == "slow_fn")
        .expect("expected slow_fn in function_deltas");

    assert_eq!(delta.kind, ChangeKind::Changed);
    assert!(delta.self_samples_delta > 0, "expected positive delta (regression)");
}

#[tokio::test]
async fn test_diff_detects_added_process() {
    let (empty_cs, empty_st, empty_fr) = FixedStore::empty_fn_tables();

    // Trace A has pid=1; trace B has pid=1 and pid=2.
    let store_a = FixedStore::new(
        empty_cs.clone(), empty_st.clone(), empty_fr.clone(),
        vec![RecordBatch::new_empty(disk_io_schema())],
        vec![make_processes(1, "app")],
        1_000_000,
    );
    let store_b = FixedStore::new(
        empty_cs, empty_st, empty_fr,
        vec![RecordBatch::new_empty(disk_io_schema())],
        vec![make_processes(1, "app"), make_processes(2, "new-daemon")],
        1_000_000,
    );

    let result = DiffEngine::new()
        .diff(&store_a, &store_b, &Selection::default(), CancellationToken::new())
        .await
        .unwrap();

    assert_eq!(result.summary.processes_added, 1, "expected 1 added process");
    assert_eq!(result.added_entities[0].pid, 2, "expected pid=2 as added entity");
}

#[tokio::test]
async fn test_diff_detects_removed_process() {
    let (empty_cs, empty_st, empty_fr) = FixedStore::empty_fn_tables();

    // Trace A has pid=1 and pid=2; trace B only has pid=1.
    let store_a = FixedStore::new(
        empty_cs.clone(), empty_st.clone(), empty_fr.clone(),
        vec![RecordBatch::new_empty(disk_io_schema())],
        vec![make_processes(1, "app"), make_processes(2, "gone-daemon")],
        1_000_000,
    );
    let store_b = FixedStore::new(
        empty_cs, empty_st, empty_fr,
        vec![RecordBatch::new_empty(disk_io_schema())],
        vec![make_processes(1, "app")],
        1_000_000,
    );

    let result = DiffEngine::new()
        .diff(&store_a, &store_b, &Selection::default(), CancellationToken::new())
        .await
        .unwrap();

    assert_eq!(result.summary.processes_removed, 1);
    assert_eq!(result.removed_entities[0].pid, 2);
}

#[tokio::test]
async fn test_diff_io_delta() {
    // Trace A: pid=1 reads 4 KB. Trace B: pid=1 reads 64 KB.
    let (empty_cs, empty_st, empty_fr) = FixedStore::empty_fn_tables();

    let store_a = FixedStore::new(
        empty_cs.clone(), empty_st.clone(), empty_fr.clone(),
        vec![make_disk_io(1, 4096, 1_000_000)],
        vec![make_processes(1, "app")],
        1_000_000,
    );
    let store_b = FixedStore::new(
        empty_cs, empty_st, empty_fr,
        vec![make_disk_io(1, 65536, 5_000_000)],
        vec![make_processes(1, "app")],
        1_000_000,
    );

    let result = DiffEngine::new()
        .diff(&store_a, &store_b, &Selection::default(), CancellationToken::new())
        .await
        .unwrap();

    let io = result
        .io_deltas
        .iter()
        .find(|d| d.pid == 1)
        .expect("expected io delta for pid=1");

    assert_eq!(io.kind, ChangeKind::Changed);
    assert_eq!(io.io_bytes_delta, 65536 - 4096);
}

#[tokio::test]
async fn test_diff_identical_traces_minimal_deltas() {
    let (cs, st, fr) = make_fn_data(1, "fn_x", 5);
    let store = FixedStore::new(
        vec![cs], vec![st], vec![fr],
        vec![RecordBatch::new_empty(disk_io_schema())],
        vec![make_processes(1, "app")],
        1_000_000,
    );

    let result = DiffEngine::new()
        .diff(&store, &store, &Selection::default(), CancellationToken::new())
        .await
        .unwrap();

    for delta in &result.function_deltas {
        assert_eq!(
            delta.self_samples_delta, 0,
            "identical traces should have zero self delta for {}",
            delta.function_name
        );
    }
    assert!(result.added_entities.is_empty());
    assert!(result.removed_entities.is_empty());
}

#[tokio::test]
async fn test_diff_cancellation() {
    let (cs, st, fr) = make_fn_data(1, "fn_x", 5);
    let store = FixedStore::new(
        vec![cs], vec![st], vec![fr],
        vec![RecordBatch::new_empty(disk_io_schema())],
        vec![make_processes(1, "app")],
        1_000_000,
    );

    let token = CancellationToken::new();
    token.cancel();

    let result = DiffEngine::new()
        .diff(&store, &store, &Selection::default(), token)
        .await;

    assert!(
        matches!(result, Err(diff::DiffError::Cancelled)),
        "expected Cancelled, got {result:?}"
    );
}
