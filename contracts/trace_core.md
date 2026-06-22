# Contract: trace-core → normalizer

**Owner:** symbols-normalize stream  
**Consumer:** normalizer crate (`src-tauri/crates/normalizer`)  
**Authoritative Rust definition:** `src-tauri/crates/trace-core/src/event.rs`

This document tracks the interface between trace-core's capture side and the
normalizer.  The Rust types are the ground truth; this doc explains the contract.

## RawEvent variants (trace-core v0.1.0)

| Variant | Key fields | Normalizer action |
|---|---|---|
| `CpuSample` | timestamp_ns (i64), process_id, thread_id, cpu_id, stack_id? | clock correct |
| `Scheduling` | timestamp_ns, prev/next pid/tid, event_type, duration_ns? | clock correct |
| `DiskIo` | timestamp_ns, pid, tid, operation, sector, size_bytes | clock correct |
| `FileIo` | timestamp_ns, pid, tid, operation, fd?, path_hash?, size_bytes? | clock correct |
| `Memory` | timestamp_ns, pid, tid?, event_type, address?, size_bytes? | clock correct |
| `Network` | timestamp_ns, pid, tid?, operation, protocol, size_bytes? | clock correct |
| `Process` | start_time_ns / exit_time_ns (both corrected), name | clock correct both |
| `Thread` | start_time_ns / exit_time_ns (both corrected), name? | clock correct both |
| `Frame` | frame_id, address, symbol_name?, module_name?, file_path?, line_number? | **symbolicate** |
| `StackEntry` | stack_id, depth, frame_id | pass through unchanged |

## Timestamp contract

- All `timestamp_ns` fields are `i64` nanoseconds since an arbitrary per-session epoch.
- The normalizer corrects backwards jumps globally (single monotone clock per session).
- `Process.exit_time_ns` and `Thread.exit_time_ns` are also corrected.

## Frame symbolication

When `Frame.symbol_name` is `None`, the normalizer looks up `Frame.address` via
the `Symbolicator` and fills in `symbol_name`, `module_name`, `file_path`,
`line_number` when available.  If the symbolicator has no module map for the
address, the frame passes through unchanged.

## Module registration

Capture backends must call `NormalizingRecorder::register_module(ModuleEntry)`
when a module is mapped into a process's address space.  Without module entries,
frame address to symbol resolution degrades to unknown (frame passes through).

## Normalizer output

`NormalizingRecorder<R>` implements `trace_core::Recorder`.  The inner `R`
(typically `StandardRecorder`) writes the corrected events to a `TraceStore`
as per-domain Arrow/Parquet tables.
