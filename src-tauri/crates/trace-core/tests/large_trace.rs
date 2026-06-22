/// Verifies that recording, saving, and re-opening a 1 M+ event trace does not
/// OOM and that the row count is preserved exactly.
use trace_core::{
    event::CpuSampleEvent, Manifest, RawEvent, Recorder, StandardRecorder, TableKind, TraceStore,
    DEFAULT_BATCH_SIZE,
};

const ONE_MILLION: usize = 1_000_000;

fn make_manifest() -> Manifest {
    Manifest::new("large-trace-test", 0_i64)
}

#[test]
fn one_million_cpu_samples_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("large.patrace");

    let mut rec = StandardRecorder::new(TraceStore::new(make_manifest()), DEFAULT_BATCH_SIZE);

    for i in 0..ONE_MILLION {
        rec.record(RawEvent::CpuSample(CpuSampleEvent {
            timestamp_ns:  i as i64,
            process_id:    (i % 16) as u32,
            thread_id:     (i % 64) as u32,
            cpu_id:        (i % 8) as u32,
            sample_weight: 1,
            stack_id:      Some((i % 1024) as u64),
        }))
        .unwrap();
    }

    let store = rec.finish().unwrap();
    assert_eq!(store.row_count(TableKind::CpuSamples), ONE_MILLION);

    // Expect ~16 row-groups (1M / 65536 ≈ 16 batches).
    let batches = store.batches(TableKind::CpuSamples);
    assert!(batches.len() >= 15, "expected ≥15 batches, got {}", batches.len());

    store.save(&path).unwrap();

    // Re-open and verify total row count without materialising all at once.
    let loaded = TraceStore::open(&path).unwrap();
    assert_eq!(loaded.row_count(TableKind::CpuSamples), ONE_MILLION);
}
