use trace_core::{
    event::{CpuSampleEvent, DiskIoEvent, DiskOp, NetworkEvent, NetworkOp, ProcessInfoEvent, Protocol},
    Manifest, RawEvent, Recorder, StandardRecorder, TableKind, TraceStore, DEFAULT_BATCH_SIZE,
};

fn minimal_manifest() -> Manifest {
    Manifest::new("test-trace-0001", 1_700_000_000_000_000_000_i64)
}

// ── empty trace ───────────────────────────────────────────────────────────────

#[test]
fn empty_trace_saves_and_reopens() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("empty.patrace");

    let store = TraceStore::new(minimal_manifest());
    store.save(&path).unwrap();

    assert!(path.join("manifest.json").exists());

    let loaded = TraceStore::open(&path).unwrap();
    assert_eq!(loaded.row_count(TableKind::CpuSamples), 0);
    assert_eq!(loaded.row_count(TableKind::Scheduling), 0);
    assert_eq!(loaded.row_count(TableKind::DiskIo), 0);
    assert_eq!(loaded.row_count(TableKind::Network), 0);
}

// ── cpu_samples round-trip ────────────────────────────────────────────────────

#[test]
fn cpu_samples_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("trace.patrace");

    let mut rec = StandardRecorder::new(TraceStore::new(minimal_manifest()), DEFAULT_BATCH_SIZE);

    for i in 0u64..100 {
        rec.record(RawEvent::CpuSample(CpuSampleEvent {
            timestamp_ns:  i as i64 * 1_000,
            process_id:    1,
            thread_id:     2,
            cpu_id:        0,
            sample_weight: 1,
            stack_id:      Some(i / 10),
        }))
        .unwrap();
    }

    let store = rec.finish().unwrap();
    assert_eq!(store.row_count(TableKind::CpuSamples), 100);

    store.save(&path).unwrap();

    let loaded = TraceStore::open(&path).unwrap();
    assert_eq!(loaded.row_count(TableKind::CpuSamples), 100);
    assert_eq!(loaded.row_count(TableKind::DiskIo), 0);

    // Verify first batch has the right schema.
    let batches = loaded.batches(TableKind::CpuSamples);
    assert!(!batches.is_empty());
    assert_eq!(
        batches[0].schema(),
        TraceStore::table_schema(TableKind::CpuSamples)
    );
}

// ── multi-domain round-trip ───────────────────────────────────────────────────

#[test]
fn multi_domain_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("multi.patrace");

    let mut rec = StandardRecorder::new(TraceStore::new(minimal_manifest()), DEFAULT_BATCH_SIZE);

    rec.record(RawEvent::CpuSample(CpuSampleEvent {
        timestamp_ns: 1_000, process_id: 100, thread_id: 200,
        cpu_id: 0, sample_weight: 1, stack_id: None,
    })).unwrap();

    rec.record(RawEvent::DiskIo(DiskIoEvent {
        timestamp_ns: 2_000, process_id: 100, thread_id: 200,
        operation: DiskOp::Read, device_major: 8, device_minor: 1,
        sector: 4096, size_bytes: 512, duration_ns: Some(1_500),
    })).unwrap();

    rec.record(RawEvent::Process(ProcessInfoEvent {
        process_id: 100, parent_process_id: Some(1),
        name: "bash".into(), cmdline: Some("/bin/bash".into()),
        start_time_ns: 0, exit_time_ns: None, exit_code: None,
    })).unwrap();

    rec.record(RawEvent::Network(NetworkEvent {
        timestamp_ns: 3_000, process_id: 100, thread_id: Some(200),
        operation: NetworkOp::Send, protocol: Protocol::Tcp, fd: Some(5),
        local_addr: Some([127, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]),
        remote_addr: None, local_port: Some(8080), remote_port: Some(443),
        size_bytes: Some(1024), duration_ns: Some(200),
    })).unwrap();

    let store = rec.finish().unwrap();
    store.save(&path).unwrap();

    let loaded = TraceStore::open(&path).unwrap();
    assert_eq!(loaded.row_count(TableKind::CpuSamples), 1);
    assert_eq!(loaded.row_count(TableKind::DiskIo), 1);
    assert_eq!(loaded.row_count(TableKind::Processes), 1);
    assert_eq!(loaded.row_count(TableKind::Network), 1);
    assert_eq!(loaded.row_count(TableKind::Memory), 0);
}

// ── version guard ─────────────────────────────────────────────────────────────

#[test]
fn future_schema_version_is_rejected() {
    use trace_core::{Error, FORMAT_VERSION};
    use std::io::Write as IoWrite;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("future.patrace");
    std::fs::create_dir_all(&path).unwrap();

    // Write a manifest claiming a schema version we don't support.
    let manifest_json = serde_json::json!({
        "format_version": FORMAT_VERSION,
        "schema_version": 9999,
        "trace_id": "future",
        "created_at_ns": 0_i64,
        "tables": []
    });
    let mut f = std::fs::File::create(path.join("manifest.json")).unwrap();
    f.write_all(manifest_json.to_string().as_bytes()).unwrap();

    let err = TraceStore::open(&path).unwrap_err();
    assert!(
        matches!(err, Error::SchemaVersionTooNew { found: 9999, .. }),
        "expected SchemaVersionTooNew, got {err:?}"
    );
}

#[test]
fn future_format_version_is_rejected() {
    use trace_core::{Error, SCHEMA_VERSION};
    use std::io::Write as IoWrite;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("future_fmt.patrace");
    std::fs::create_dir_all(&path).unwrap();

    let manifest_json = serde_json::json!({
        "format_version": 9999_u32,
        "schema_version": SCHEMA_VERSION,
        "trace_id": "future",
        "created_at_ns": 0_i64,
        "tables": []
    });
    let mut f = std::fs::File::create(path.join("manifest.json")).unwrap();
    f.write_all(manifest_json.to_string().as_bytes()).unwrap();

    let err = TraceStore::open(&path).unwrap_err();
    assert!(
        matches!(err, Error::FormatVersionTooNew { found: 9999, .. }),
        "expected FormatVersionTooNew, got {err:?}"
    );
}

// ── NoOpRecorder ──────────────────────────────────────────────────────────────

#[test]
fn noop_recorder_accepts_any_event() {
    use trace_core::NoOpRecorder;

    let mut rec = NoOpRecorder;
    rec.record(RawEvent::CpuSample(CpuSampleEvent {
        timestamp_ns: 0, process_id: 1, thread_id: 1,
        cpu_id: 0, sample_weight: 1, stack_id: None,
    })).unwrap();
    rec.flush().unwrap();
}
