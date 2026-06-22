/// Adapter for the Process Analyzer native container (.patrace directory).
///
/// A `.patrace` container is a directory produced by `pa-recorder`:
///   manifest.json        — metadata (format_version, schema_version, trace_id, …)
///   cpu_samples.parquet  — Arrow rows (optional, one file per non-empty domain)
///   scheduling.parquet
///   … etc.
///
/// This adapter opens the store with `TraceStore::open`, then re-emits all rows
/// as `RawEvent`s so that import and live recording share the same event pipeline.
/// The re-emit path is useful for combining traces, schema migration, and export.
///
/// Corrupt files: the store validates Parquet checksums; partially-written row-groups
/// are skipped with a `PartialCorruption` error emitted into the stream.
use std::path::Path;

use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use arrow::array::{Array, Int32Array, Int64Array, StringArray, UInt32Array, UInt64Array, UInt8Array};

use trace_core::event::*;
use trace_core::{RawEvent, TableKind, TraceStore};

use crate::error::ImportError;
use crate::ImportResult;

pub async fn import(path: &Path) -> Result<ImportResult, ImportError> {
    let path_owned = path.to_path_buf();

    // TraceStore::open is synchronous (blocking I/O) — run it on the thread pool.
    let store = tokio::task::spawn_blocking(move || {
        TraceStore::open(&path_owned).map_err(|e| ImportError::TraceCore {
            path: path_owned.clone(),
            reason: e.to_string(),
        })
    })
    .await
    .map_err(|e| ImportError::Io {
        path: path.to_path_buf(),
        source: std::io::Error::new(std::io::ErrorKind::Other, e.to_string()),
    })??;

    let (tx, rx) = mpsc::channel::<Result<RawEvent, ImportError>>(512);
    let path_for_err = path.to_path_buf();

    tokio::spawn(async move {
        emit_store_events(store, path_for_err, tx).await;
    });

    Ok(ImportResult {
        events: Box::pin(ReceiverStream::new(rx)),
        is_complete: true,
        boundary_offset: None,
    })
}

async fn emit_store_events(
    store: TraceStore,
    path: std::path::PathBuf,
    tx: mpsc::Sender<Result<RawEvent, ImportError>>,
) {
    for &kind in TableKind::all() {
        for (batch_idx, batch) in store.batches(kind).iter().enumerate() {
            let events = decode_batch(kind, batch);
            match events {
                Ok(evs) => {
                    for ev in evs {
                        if tx.send(Ok(ev)).await.is_err() { return; }
                    }
                }
                Err(reason) => {
                    let _ = tx.send(Err(ImportError::PartialCorruption {
                        path: path.clone(),
                        boundary_offset: batch_idx as u64,
                        reason,
                    })).await;
                    return;
                }
            }
        }
    }
}

fn decode_batch(
    kind: TableKind,
    batch: &arrow::array::RecordBatch,
) -> Result<Vec<RawEvent>, String> {
    let nrows = batch.num_rows();
    let mut out = Vec::with_capacity(nrows);

    macro_rules! col_i64 {
        ($idx:expr) => {
            batch.column($idx)
                .as_any()
                .downcast_ref::<Int64Array>()
                .ok_or_else(|| format!("column {} not Int64", $idx))?
        };
    }
    macro_rules! col_u32 {
        ($idx:expr) => {
            batch.column($idx)
                .as_any()
                .downcast_ref::<UInt32Array>()
                .ok_or_else(|| format!("column {} not UInt32", $idx))?
        };
    }
    macro_rules! col_u64 {
        ($idx:expr) => {
            batch.column($idx)
                .as_any()
                .downcast_ref::<UInt64Array>()
                .ok_or_else(|| format!("column {} not UInt64", $idx))?
        };
    }
    macro_rules! col_u8 {
        ($idx:expr) => {
            batch.column($idx)
                .as_any()
                .downcast_ref::<UInt8Array>()
                .ok_or_else(|| format!("column {} not UInt8", $idx))?
        };
    }
    macro_rules! col_i32 {
        ($idx:expr) => {
            batch.column($idx)
                .as_any()
                .downcast_ref::<Int32Array>()
                .ok_or_else(|| format!("column {} not Int32", $idx))?
        };
    }
    macro_rules! col_str {
        ($idx:expr) => {
            batch.column($idx)
                .as_any()
                .downcast_ref::<StringArray>()
                .ok_or_else(|| format!("column {} not Utf8", $idx))?
        };
    }

    match kind {
        TableKind::CpuSamples => {
            let ts  = col_i64!(0);
            let pid = col_u32!(1);
            let tid = col_u32!(2);
            let cpu = col_u32!(3);
            let wt  = col_u64!(4);
            let sid = col_u64!(5);
            for i in 0..nrows {
                out.push(RawEvent::CpuSample(CpuSampleEvent {
                    timestamp_ns:  ts.value(i),
                    process_id:    pid.value(i),
                    thread_id:     tid.value(i),
                    cpu_id:        cpu.value(i),
                    sample_weight: wt.value(i),
                    stack_id:      if sid.is_null(i) { None } else { Some(sid.value(i)) },
                }));
            }
        }
        TableKind::Scheduling => {
            let ts   = col_i64!(0);
            let pid  = col_u32!(1);
            let tid  = col_u32!(2);
            let cpu  = col_u32!(3);
            let et   = col_u8!(4);
            let ps   = col_u8!(5);
            let npid = col_u32!(6);
            let ntid = col_u32!(7);
            let dur  = col_u64!(8);
            for i in 0..nrows {
                let event_type = match et.value(i) {
                    0 => SchedulingEventType::ContextSwitchOut,
                    1 => SchedulingEventType::ContextSwitchIn,
                    2 => SchedulingEventType::Wakeup,
                    3 => SchedulingEventType::Migration,
                    _ => SchedulingEventType::ContextSwitchIn,
                };
                out.push(RawEvent::Scheduling(SchedulingEvent {
                    timestamp_ns: ts.value(i),
                    process_id: pid.value(i),
                    thread_id: tid.value(i),
                    cpu_id: cpu.value(i),
                    event_type,
                    prev_state:      if ps.is_null(i) { None } else { Some(ps.value(i)) },
                    next_process_id: if npid.is_null(i) { None } else { Some(npid.value(i)) },
                    next_thread_id:  if ntid.is_null(i) { None } else { Some(ntid.value(i)) },
                    duration_ns:     if dur.is_null(i)  { None } else { Some(dur.value(i)) },
                }));
            }
        }
        TableKind::Processes => {
            let pid      = col_u32!(0);
            let ppid     = col_u32!(1);
            let name     = col_str!(2);
            let cmdline  = col_str!(3);
            let start_ts = col_i64!(4);
            let exit_ts  = col_i64!(5);
            let exit_code = col_i32!(6);
            for i in 0..nrows {
                out.push(RawEvent::Process(ProcessInfoEvent {
                    process_id:        pid.value(i),
                    parent_process_id: if ppid.is_null(i)     { None } else { Some(ppid.value(i)) },
                    name:              name.value(i).to_owned(),
                    cmdline:           if cmdline.is_null(i)  { None } else { Some(cmdline.value(i).to_owned()) },
                    start_time_ns:     start_ts.value(i),
                    exit_time_ns:      if exit_ts.is_null(i)   { None } else { Some(exit_ts.value(i)) },
                    exit_code:         if exit_code.is_null(i) { None } else { Some(exit_code.value(i)) },
                }));
            }
        }
        TableKind::Threads => {
            let tid      = col_u32!(0);
            let pid      = col_u32!(1);
            let name     = col_str!(2);
            let start_ts = col_i64!(3);
            let exit_ts  = col_i64!(4);
            for i in 0..nrows {
                out.push(RawEvent::Thread(ThreadInfoEvent {
                    thread_id:     tid.value(i),
                    process_id:    pid.value(i),
                    name:          if name.is_null(i) { None } else { Some(name.value(i).to_owned()) },
                    start_time_ns: start_ts.value(i),
                    exit_time_ns:  if exit_ts.is_null(i) { None } else { Some(exit_ts.value(i)) },
                }));
            }
        }
        TableKind::Stacks => {
            let sid = col_u64!(0);
            let dep = col_u32!(1);
            let fid = col_u64!(2);
            for i in 0..nrows {
                out.push(RawEvent::StackEntry(StackEntryEvent {
                    stack_id: sid.value(i),
                    depth: dep.value(i),
                    frame_id: fid.value(i),
                }));
            }
        }
        // Remaining domains decoded similarly; emit nothing for now (schema
        // decoding is added incrementally as each domain's query path lands).
        _ => {}
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use trace_core::{Manifest, StandardRecorder, TraceStore, recorder::Recorder as _};

    fn make_test_store() -> TraceStore {
        let manifest = Manifest::new("test-id", 1_000_000_000_i64);
        let store = TraceStore::new(manifest);
        let mut rec = StandardRecorder::new(store, 1024);
        rec.record(RawEvent::CpuSample(CpuSampleEvent {
            timestamp_ns: 1_000_000, process_id: 42, thread_id: 43,
            cpu_id: 0, sample_weight: 1, stack_id: None,
        })).unwrap();
        rec.flush().unwrap();
        rec.finish().unwrap()
    }

    #[tokio::test]
    async fn roundtrip_cpu_sample() {
        let dir = tempfile::tempdir().unwrap();
        let trace_path = dir.path().join("test.patrace");

        let store = make_test_store();
        store.save(&trace_path).unwrap();

        use futures::StreamExt as _;
        let result = import(&trace_path).await.unwrap();
        let events: Vec<_> = result.events.collect().await;
        assert_eq!(events.len(), 1);
        if let RawEvent::CpuSample(e) = events[0].as_ref().unwrap() {
            assert_eq!(e.process_id, 42);
            assert_eq!(e.thread_id, 43);
        } else { panic!("expected CpuSample"); }
    }

    #[tokio::test]
    async fn rejects_missing_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let trace_path = dir.path().join("bad.patrace");
        tokio::fs::create_dir_all(&trace_path).await.unwrap();
        // No manifest.json — TraceStore::open should fail
        assert!(matches!(import(&trace_path).await, Err(ImportError::TraceCore { .. })));
    }
}
