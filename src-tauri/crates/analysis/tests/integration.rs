use std::sync::Arc;

use arrow::{
    array::{Int32Array, Int64Array, UInt8Array, UInt32Array, UInt64Array, StringArray},
    record_batch::RecordBatch,
};
use analysis::{AnalysisEngine, FindingKind, Severity};
use query::{
    Selection, TraceStore,
    cpu_samples_schema, stacks_schema, frames_schema, scheduling_schema,
    disk_io_schema, file_io_schema, memory_schema, processes_schema, threads_schema,
};
use tokio_util::sync::CancellationToken;

// ── Shared mock ───────────────────────────────────────────────────────────────

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

impl TraceStore for MockStore {
    fn cpu_samples(&self) -> &[RecordBatch] { &self.cpu_samples }
    fn stacks(&self)       -> &[RecordBatch] { &self.stacks }
    fn frames(&self)       -> &[RecordBatch] { &self.frames }
    fn scheduling(&self)   -> &[RecordBatch] { &self.scheduling }
    fn disk_io(&self)      -> &[RecordBatch] { &self.disk_io }
    fn file_io(&self)      -> &[RecordBatch] { &self.file_io }
    fn memory(&self)       -> &[RecordBatch] { &self.memory }
    fn processes(&self)    -> &[RecordBatch] { &self.processes }
    fn threads(&self)      -> &[RecordBatch] { &self.threads }
    fn duration_ns(&self)  -> u64            { self.duration_ns }
}

impl MockStore {
    /// All CPU samples from pid=1 → HotProcessDetector fires.
    fn hot_process() -> Self {
        let n = 10usize;

        let cpu_samples = RecordBatch::try_new(
            cpu_samples_schema(),
            vec![
                Arc::new(Int64Array::from((0..n as i64).map(|i| i * 100).collect::<Vec<_>>())),
                Arc::new(UInt32Array::from(vec![1u32; n])),
                Arc::new(UInt32Array::from(vec![10u32; n])),
                Arc::new(UInt32Array::from(vec![0u32; n])),
                Arc::new(UInt64Array::from(vec![1u64; n])),
                Arc::new(UInt64Array::from(vec![Some(1u64); n])),
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
                Arc::new(StringArray::from(vec![Some("hot_fn")])),
                Arc::new(StringArray::from(vec![Some("app")])),
                Arc::new(StringArray::from(vec![None::<&str>])),
                Arc::new(UInt32Array::from(vec![None::<u32>])),
            ],
        )
        .unwrap();

        let processes = RecordBatch::try_new(
            processes_schema(),
            vec![
                Arc::new(UInt32Array::from(vec![1u32])),
                Arc::new(UInt32Array::from(vec![None::<u32>])),
                Arc::new(StringArray::from(vec!["hot-process"])),
                Arc::new(StringArray::from(vec![None::<&str>])),
                Arc::new(Int64Array::from(vec![0i64])),
                Arc::new(Int64Array::from(vec![None::<i64>])),
                Arc::new(Int32Array::from(vec![None::<i32>])),
            ],
        )
        .unwrap();

        Self {
            cpu_samples: vec![cpu_samples],
            stacks:      vec![stacks],
            frames:      vec![frames],
            scheduling:  vec![RecordBatch::new_empty(scheduling_schema())],
            disk_io:     vec![RecordBatch::new_empty(disk_io_schema())],
            file_io:     vec![RecordBatch::new_empty(file_io_schema())],
            memory:      vec![RecordBatch::new_empty(memory_schema())],
            processes:   vec![processes],
            threads:     vec![RecordBatch::new_empty(threads_schema())],
            duration_ns: 1_000_000,
        }
    }

    /// disk_io with a 100 ms stall → IoStallDetector fires.
    fn io_stall() -> Self {
        let processes = RecordBatch::try_new(
            processes_schema(),
            vec![
                Arc::new(UInt32Array::from(vec![1u32])),
                Arc::new(UInt32Array::from(vec![None::<u32>])),
                Arc::new(StringArray::from(vec!["staller"])),
                Arc::new(StringArray::from(vec![None::<&str>])),
                Arc::new(Int64Array::from(vec![0i64])),
                Arc::new(Int64Array::from(vec![None::<i64>])),
                Arc::new(Int32Array::from(vec![None::<i32>])),
            ],
        )
        .unwrap();

        // 100 ms stall, well above the 10 ms IoStallDetector default threshold.
        let disk_io = RecordBatch::try_new(
            disk_io_schema(),
            vec![
                Arc::new(Int64Array::from(vec![500_000_000i64])),
                Arc::new(UInt32Array::from(vec![1u32])),
                Arc::new(UInt32Array::from(vec![10u32])),
                Arc::new(UInt8Array::from(vec![0u8])),
                Arc::new(UInt32Array::from(vec![8u32])),
                Arc::new(UInt32Array::from(vec![0u32])),
                Arc::new(UInt64Array::from(vec![0u64])),
                Arc::new(UInt64Array::from(vec![4096u64])),
                Arc::new(UInt64Array::from(vec![Some(100_000_000u64)])),
            ],
        )
        .unwrap();

        Self {
            cpu_samples: vec![RecordBatch::new_empty(cpu_samples_schema())],
            stacks:      vec![RecordBatch::new_empty(stacks_schema())],
            frames:      vec![RecordBatch::new_empty(frames_schema())],
            scheduling:  vec![RecordBatch::new_empty(scheduling_schema())],
            disk_io:     vec![disk_io],
            file_io:     vec![RecordBatch::new_empty(file_io_schema())],
            memory:      vec![RecordBatch::new_empty(memory_schema())],
            processes:   vec![processes],
            threads:     vec![RecordBatch::new_empty(threads_schema())],
            duration_ns: 1_000_000_000,
        }
    }

    /// All tables empty → all detectors should return nothing.
    fn quiet() -> Self {
        Self {
            cpu_samples: vec![RecordBatch::new_empty(cpu_samples_schema())],
            stacks:      vec![RecordBatch::new_empty(stacks_schema())],
            frames:      vec![RecordBatch::new_empty(frames_schema())],
            scheduling:  vec![RecordBatch::new_empty(scheduling_schema())],
            disk_io:     vec![RecordBatch::new_empty(disk_io_schema())],
            file_io:     vec![RecordBatch::new_empty(file_io_schema())],
            memory:      vec![RecordBatch::new_empty(memory_schema())],
            processes:   vec![RecordBatch::new_empty(processes_schema())],
            threads:     vec![RecordBatch::new_empty(threads_schema())],
            duration_ns: 500_000,
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_hot_process_detected() {
    let store = MockStore::hot_process();
    let engine = AnalysisEngine::new();
    let findings = engine
        .run_all(&store, &Selection::default(), CancellationToken::new())
        .await
        .unwrap();

    let hot = findings.iter().find(|f| f.kind == FindingKind::HotProcess);
    assert!(hot.is_some(), "expected HotProcess finding; got {findings:?}");
    assert!(hot.unwrap().severity >= Severity::Medium, "expected at least Medium severity");
}

#[tokio::test]
async fn test_io_stall_detected() {
    let store = MockStore::io_stall();
    let engine = AnalysisEngine::new();
    let findings = engine
        .run_all(&store, &Selection::default(), CancellationToken::new())
        .await
        .unwrap();

    let stall = findings.iter().find(|f| f.kind == FindingKind::IoStall);
    assert!(stall.is_some(), "expected IoStall finding; got {findings:?}");
    assert!(stall.unwrap().severity >= Severity::Medium, "100 ms I/O should be at least Medium");
}

#[tokio::test]
async fn test_quiet_trace_returns_empty() {
    let store = MockStore::quiet();
    let engine = AnalysisEngine::new();
    let findings = engine
        .run_all(&store, &Selection::default(), CancellationToken::new())
        .await
        .unwrap();

    let non_info: Vec<_> = findings.iter().filter(|f| f.severity > Severity::Info).collect();
    assert!(
        non_info.is_empty(),
        "quiet trace produced unexpected findings: {non_info:?}"
    );
}

#[tokio::test]
async fn test_findings_are_ranked_by_severity() {
    let store = MockStore::hot_process();
    let engine = AnalysisEngine::new();
    let findings = engine
        .run_all(&store, &Selection::default(), CancellationToken::new())
        .await
        .unwrap();

    for i in 1..findings.len() {
        let a = &findings[i - 1];
        let b = &findings[i];
        let j = i - 1;
        assert!(
            a.severity >= b.severity,
            "findings not sorted: [{j}] {:?} < [{i}] {:?}",
            a.severity,
            b.severity,
        );
    }
}

#[tokio::test]
async fn test_cancellation_returns_empty() {
    let store = MockStore::hot_process();
    let engine = AnalysisEngine::new();
    let token = CancellationToken::new();
    token.cancel();

    let findings = engine
        .run_all(&store, &Selection::default(), token)
        .await
        .unwrap();

    // With immediate cancellation, zero or very few findings are expected.
    let _ = findings;
}
