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

For frame symbolication to work, the normalizer's `Symbolicator` must know the
process module map.  There are two ways to populate it:

### A – `RawEvent::ModuleLoad` (preferred, requires trace-core change)

Add two new variants to `trace_core::RawEvent`:

```rust
ModuleLoad(ModuleLoadEvent),
ModuleUnload(ModuleUnloadEvent),
```

```rust
pub struct ModuleLoadEvent {
    pub timestamp_ns: i64,
    pub process_id: u32,
    pub base_address: u64,
    pub size: u64,
    pub path: String,
    /// Build ID: ELF build-id, Mach-O LC_UUID, or Windows PDB GUID+age
    pub build_id: Option<Vec<u8>>,
}

pub struct ModuleUnloadEvent {
    pub timestamp_ns: i64,
    pub process_id: u32,
    pub base_address: u64,
}
```

The normalizer will intercept these in its `Recorder::record()` implementation,
call `register_module()`/`unregister_module()`, and **not** forward them to the
inner recorder (they are normalizer-internal bookkeeping, not recorded metrics).

**Status: pending — trace-core stream must add these variants.**

The Windows ETW consumer already collects `IMAGE_LOAD` events via
`ImageLoadGuid {2cb15d1d-5fc1-11d2-abe1-00a0c911f518}` opcode 10 (see
`backend-windows/src/etw/consumer.rs:132`). The macOS backend already collects
`DyldImage { load_address, path, uuid }` data (see
`backend-macos/src/types.rs`). Both backends need to emit these events once the
variant exists.

### B – Direct API call (workaround until variant lands)

A caller that has direct access to the `NormalizingRecorder` (not via channel)
can call:

```rust
rec.register_module(ModuleEntry { base, size, path, build_id });
rec.unregister_module(base);
```

This is only feasible when the same Rust call site owns both the recorder and
the module-load notification — e.g., a single-process integration test.

## Normalizer output

`NormalizingRecorder<R>` implements `trace_core::Recorder`.  The inner `R`
(typically `StandardRecorder`) writes the corrected events to a `TraceStore`
as per-domain Arrow/Parquet tables.
