/**
 * Query engine contract — owned by query-analysis stream.
 * Defines the shape of data returned by Tauri commands and events.
 * The query-analysis Rust commands must return data matching these types.
 */

// ─── Process table ──────────────────────────────────────────────────────────

export interface ProcessRow {
  pid: number;
  name: string;
  /** 0–100, sampled CPU share */
  cpuPercent: number;
  /** Resident set size in bytes */
  memoryBytes: number;
  ioReadBytes: number;
  ioWriteBytes: number;
  networkRxBytes: number;
  networkTxBytes: number;
  threadCount: number;
  /** Whether capture includes data for this domain; false → column shows N/A */
  hasCpu: boolean;
  hasMemory: boolean;
  hasIo: boolean;
  hasNetwork: boolean;
}

// ─── Thread table ────────────────────────────────────────────────────────────

export type ThreadState = 'running' | 'sleeping' | 'blocked' | 'idle' | 'zombie' | 'unknown';

export interface ThreadRow {
  tid: number;
  name: string;
  cpuPercent: number;
  state: ThreadState;
  stackDepth: number;
  waitReason?: string;
  hasCpu: boolean;
}

// ─── Details ─────────────────────────────────────────────────────────────────

export interface ProcessDetails {
  pid: number;
  name: string;
  path?: string;
  cmdline?: string[];
  /** Nanoseconds since epoch */
  startTimeNs: number;
  cpuPercent: number;
  memoryBytes: number;
  ioReadBytes: number;
  ioWriteBytes: number;
  networkRxBytes: number;
  networkTxBytes: number;
  threadCount: number;
}

export interface ThreadDetails {
  tid: number;
  name: string;
  state: ThreadState;
  cpuPercent: number;
  stackDepth: number;
  waitReason?: string;
  pid: number;
}

// ─── Stack ───────────────────────────────────────────────────────────────────

export interface SymbolizedFrame {
  /** Instruction pointer */
  address: number;
  /** Resolved symbol name, or hex address if unresolved */
  symbol: string;
  file?: string;
  line?: number;
  column?: number;
  /** True when this frame was inlined by the compiler */
  inlined: boolean;
  /** Module (shared lib / exe) that contains this frame */
  module?: string;
}

// ─── Findings ────────────────────────────────────────────────────────────────

export type FindingSeverity = 'critical' | 'high' | 'medium' | 'low' | 'info';

export interface Finding {
  id: string;
  severity: FindingSeverity;
  title: string;
  description: string;
  /** Affected process — if set, clicking jumps there */
  pid?: number;
  /** Affected thread — if set, clicking jumps there */
  tid?: number;
  /** Hottest frame address, if applicable */
  address?: number;
  category: string;
  /** Nanosecond timestamp of the event that triggered this finding */
  timestampNs?: number;
}

// ─── Presets ─────────────────────────────────────────────────────────────────

/**
 * A preset is a named investigation configuration.
 * Applying one sets filters/sorts non-destructively; revert restores prior state.
 */
export interface PresetConfig {
  id: string;
  label: string;
  description: string;
  /** Initial sort for the process table when this preset is applied */
  processSort: { column: keyof ProcessRow; desc: boolean }[];
  threadSort: { column: keyof ThreadRow; desc: boolean }[];
  /** Optional filter — only show processes/threads matching this predicate */
  processFilter?: (row: ProcessRow) => boolean;
  threadFilter?: (row: ThreadRow) => boolean;
}

// ─── Tauri command signatures (for invoke typing) ────────────────────────────

/**
 * `get_process_aggregates` → Arrow IPC bytes (Uint8Array).
 * Decoded columns match ProcessRow fields.
 */
export type GetProcessAggregatesCmd = 'get_process_aggregates';

/**
 * `get_thread_aggregates` → Arrow IPC bytes (Uint8Array).
 * Decoded columns match ThreadRow fields.
 */
export type GetThreadAggregatesCmd = 'get_thread_aggregates';
export interface GetThreadAggregatesArgs { pid: number }

/** `get_process_details` → ProcessDetails (JSON) */
export type GetProcessDetailsCmd = 'get_process_details';
export interface GetProcessDetailsArgs { pid: number }

/** `get_thread_details` → ThreadDetails (JSON) */
export type GetThreadDetailsCmd = 'get_thread_details';
export interface GetThreadDetailsArgs { pid: number; tid: number }

/** `get_thread_stack` → SymbolizedFrame[] (JSON) */
export type GetThreadStackCmd = 'get_thread_stack';
export interface GetThreadStackArgs { pid: number; tid: number }

/** Tauri event name for findings emitted by the analysis engine */
export const FINDING_EVENT = 'finding' as const;
