/**
 * IPC contract for viz-panes Tauri commands.
 *
 * These command signatures must be added to src/contracts/query-engine.ts
 * (owned by query-analysis stream) and implemented in the Rust backend
 * (owned by query-analysis stream) at integration time.
 *
 * All commands return an ArrayBuffer containing an Arrow IPC stream;
 * parse with `tableFromIPC(new Uint8Array(buf))` from apache-arrow.
 */

// Re-export TimeRange from the central contract to avoid dual imports.
export type { TimeRange } from '../contracts/selection-store';

// ─── Timeline events ─────────────────────────────────────────────────────────

/**
 * `query_timeline_events` → Arrow IPC
 *
 * Arrow schema:
 *   time_ns: int64       — event start, nanoseconds since trace epoch
 *   duration_ns: int64   — event duration in nanoseconds
 *   pid: int32
 *   tid: int32
 *   name: utf8           — event/function name
 *   kind: utf8           — 'cpu' | 'io' | 'syscall' | 'user'
 *   depth: int32         — nesting depth (0 = top-level)
 */
export type QueryTimelineEventsCmd = 'query_timeline_events';
export interface QueryTimelineEventsArgs {
  pid: number | null;
  tid: number | null;
  timeRangeNs: { startNs: number; endNs: number } | null;
}

// ─── Call tree ───────────────────────────────────────────────────────────────

/**
 * `query_call_tree` → Arrow IPC
 *
 * Arrow schema:
 *   id: int32
 *   parent_id: int32     — -1 for root
 *   frame: utf8          — symbolized function name
 *   self_ns: int64       — exclusive time
 *   total_ns: int64      — inclusive time
 *   depth: int32
 */
export type QueryCallTreeCmd = 'query_call_tree';
export interface QueryCallTreeArgs {
  pid: number | null;
  tid: number | null;
  timeRangeNs: { startNs: number; endNs: number } | null;
}

// ─── Call tree diff ──────────────────────────────────────────────────────────

/**
 * `query_call_tree_diff` → Arrow IPC
 *
 * Arrow schema:
 *   id: int32
 *   parent_id: int32
 *   frame: utf8
 *   baseline_ns: int64
 *   regression_ns: int64
 *   delta_pct: float64   — (regression - baseline) / baseline * 100
 *   depth: int32
 */
export type QueryCallTreeDiffCmd = 'query_call_tree_diff';
export interface QueryCallTreeDiffArgs {
  baselineTraceId: string;
  regressionTraceId: string;
  pid: number | null;
  tid: number | null;
  timeRangeNs: { startNs: number; endNs: number } | null;
}
