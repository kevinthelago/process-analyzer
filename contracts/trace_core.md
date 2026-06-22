# Contract: trace-core → normalizer

**Owner:** symbols-normalize stream  
**Consumer:** normalizer crate (`src-tauri/crates/normalizer`)  
**Source of truth for Rust definition:** `src-tauri/crates/normalizer/src/raw_event.rs`

## RawEvent

The capture backends (linux-backend, macos-backend, windows-backend) produce a stream of
`RawEvent` values.  The normalizer consumes this stream via `Normalizer::push(event)`.

### Variants

| Variant | Fields | Notes |
|---|---|---|
| `ProcessCreate` | pid, ppid, name, timestamp_ns | |
| `ProcessExit` | pid, exit_code, timestamp_ns | |
| `ThreadCreate` | pid, tid, name?, timestamp_ns | name is optional |
| `ThreadExit` | pid, tid, timestamp_ns | |
| `StackSample` | pid, tid, frames: Vec<u64>, timestamp_ns, cpu? | frames are raw VAs, innermost first |
| `ContextSwitch` | prev_pid, prev_tid, next_pid, next_tid, timestamp_ns, cpu? | |
| `FileRead` | pid, tid, bytes, timestamp_ns | |
| `FileWrite` | pid, tid, bytes, timestamp_ns | |
| `SyscallEnter` | pid, tid, nr, timestamp_ns | |
| `SyscallExit` | pid, tid, nr, ret, timestamp_ns | |
| `ModuleLoad` | pid, base, size, path, build_id?, timestamp_ns | triggers symbolicator registration |
| `ModuleUnload` | pid, base, timestamp_ns | |
| `Unknown` | event_type, timestamp_ns, payload | counted and dropped by normalizer |

### Timestamp contract

- All `timestamp_ns` values are nanoseconds since an arbitrary per-capture-session epoch.
- The normalizer corrects backwards jumps (TSC skew, CPU migration) to monotone.
- The normalizer does **not** require the epoch to align with wall-clock time.

### ModuleLoad / address resolution

- `base` is the virtual address at which the module was mapped.
- `size` is the size in bytes of the mapping.
- `path` is the on-disk path to the binary.
- The normalizer calls `Symbolicator::add_module` when it sees `ModuleLoad`, so that
  subsequent `StackSample` frames can be resolved.

## Arrow schema (normalizer output)

See `src-tauri/crates/normalizer/src/schema.rs` for the authoritative Arrow schema.
RecordBatches flow out of `Normalizer::push()` / `Normalizer::flush()`.
