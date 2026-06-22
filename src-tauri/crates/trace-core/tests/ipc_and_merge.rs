use trace_core::{
    event::{CpuSampleEvent, SchedulingEvent, SchedulingEventType},
    ipc, Manifest, RawEvent, Recorder, StandardRecorder, TableKind, TraceStore,
    DEFAULT_BATCH_SIZE,
};

fn manifest(id: &str) -> Manifest {
    Manifest::new(id, 0_i64)
}

fn make_store_with_cpu_samples(n: usize, base_ts: i64) -> TraceStore {
    let mut rec = StandardRecorder::new(TraceStore::new(manifest("t")), DEFAULT_BATCH_SIZE);
    for i in 0..n {
        rec.record(RawEvent::CpuSample(CpuSampleEvent {
            timestamp_ns:  base_ts + i as i64,
            process_id:    1,
            thread_id:     1,
            cpu_id:        0,
            sample_weight: 1,
            stack_id:      None,
        }))
        .unwrap();
    }
    rec.finish().unwrap()
}

// ── Arrow IPC round-trip ──────────────────────────────────────────────────────

#[test]
fn encode_decode_empty_batches() {
    let schema = TableKind::CpuSamples.schema();
    let bytes = ipc::encode_batches(schema.clone(), &[]).unwrap();
    assert!(!bytes.is_empty(), "IPC file header must be present even for empty batch list");

    let (decoded_schema, batches) = ipc::decode_batches(&bytes).unwrap();
    assert_eq!(decoded_schema, schema);
    assert_eq!(batches.len(), 0);
}

#[test]
fn encode_decode_cpu_samples() {
    let store = make_store_with_cpu_samples(500, 0);
    let batches = store.batches(TableKind::CpuSamples);
    assert_eq!(batches.len(), 1);

    let schema = TableKind::CpuSamples.schema();
    let bytes = ipc::encode_batches(schema.clone(), batches).unwrap();

    let (decoded_schema, decoded) = ipc::decode_batches(&bytes).unwrap();
    assert_eq!(decoded_schema, schema);
    assert_eq!(decoded.len(), 1);
    assert_eq!(decoded[0].num_rows(), 500);
}

#[test]
fn encode_multi_batch_decode_preserves_count() {
    let store = make_store_with_cpu_samples(200_000, 0);
    let batches = store.batches(TableKind::CpuSamples);
    assert!(batches.len() >= 3, "expected multiple batches for 200k events");

    let schema = TableKind::CpuSamples.schema();
    let bytes = ipc::encode_batches(schema.clone(), batches).unwrap();
    let (_, decoded) = ipc::decode_batches(&bytes).unwrap();

    let total: usize = decoded.iter().map(|b| b.num_rows()).sum();
    assert_eq!(total, 200_000);
}

// ── TraceStore::merge ─────────────────────────────────────────────────────────

#[test]
fn merge_two_stores_sums_row_counts() {
    let mut a = make_store_with_cpu_samples(100, 0);
    let b = make_store_with_cpu_samples(200, 1_000_000);

    a.merge(b).unwrap();
    assert_eq!(a.row_count(TableKind::CpuSamples), 300);
}

#[test]
fn merge_disjoint_domains() {
    let store_a = {
        let mut rec = StandardRecorder::new(TraceStore::new(manifest("a")), DEFAULT_BATCH_SIZE);
        rec.record(RawEvent::CpuSample(CpuSampleEvent {
            timestamp_ns: 1, process_id: 1, thread_id: 1,
            cpu_id: 0, sample_weight: 1, stack_id: None,
        })).unwrap();
        rec.finish().unwrap()
    };

    let store_b = {
        let mut rec = StandardRecorder::new(TraceStore::new(manifest("b")), DEFAULT_BATCH_SIZE);
        rec.record(RawEvent::Scheduling(SchedulingEvent {
            timestamp_ns: 2, process_id: 1, thread_id: 1, cpu_id: 0,
            event_type: SchedulingEventType::Wakeup,
            prev_state: None, next_process_id: None, next_thread_id: None,
            duration_ns: None,
        })).unwrap();
        rec.finish().unwrap()
    };

    let mut merged = store_a;
    merged.merge(store_b).unwrap();
    assert_eq!(merged.row_count(TableKind::CpuSamples), 1);
    assert_eq!(merged.row_count(TableKind::Scheduling), 1);
}

// ── TraceStore::time_range ────────────────────────────────────────────────────

#[test]
fn time_range_returns_correct_bounds() {
    let store = make_store_with_cpu_samples(100, 1_000_000);
    // timestamps: 1_000_000 .. 1_000_099
    let range = store.time_range(TableKind::CpuSamples).unwrap();
    assert_eq!(range.0, 1_000_000);
    assert_eq!(range.1, 1_000_099);
}

#[test]
fn time_range_empty_store_returns_none() {
    let store = TraceStore::new(manifest("empty"));
    assert!(store.time_range(TableKind::CpuSamples).is_none());
}

#[test]
fn time_range_metadata_table_returns_none() {
    // `frames` has no timestamp_ns column.
    let store = TraceStore::new(manifest("meta"));
    assert!(store.time_range(TableKind::Frames).is_none());
}
