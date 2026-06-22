import { tableFromIPC, Table, Schema } from 'apache-arrow';
import type { ProcessRow, ThreadRow } from '@/contracts/query-engine';

/** Deserialize an Arrow IPC payload returned by a Tauri command into a JS array. */
function arrowBytesToTable(bytes: ArrayBuffer | Uint8Array): Table {
  const buf = bytes instanceof Uint8Array ? bytes.buffer : bytes;
  return tableFromIPC(new Uint8Array(buf));
}

function getNumber(table: Table, col: string, row: number): number {
  return Number(table.getChildAt(table.schema.fields.findIndex(f => f.name === col))?.get(row) ?? 0);
}

function getString(table: Table, col: string, row: number): string {
  return String(table.getChildAt(table.schema.fields.findIndex(f => f.name === col))?.get(row) ?? '');
}

function getBool(table: Table, col: string, row: number): boolean {
  const idx = table.schema.fields.findIndex(f => f.name === col);
  if (idx === -1) return true; // default true: unknown domain availability → show
  return Boolean(table.getChildAt(idx)?.get(row) ?? true);
}

export function parseProcessTable(bytes: ArrayBuffer | Uint8Array): ProcessRow[] {
  const arrow = arrowBytesToTable(bytes);
  const rows: ProcessRow[] = [];
  for (let i = 0; i < arrow.numRows; i++) {
    rows.push({
      pid: getNumber(arrow, 'pid', i),
      name: getString(arrow, 'name', i),
      cpuPercent: getNumber(arrow, 'cpu_percent', i),
      memoryBytes: getNumber(arrow, 'memory_bytes', i),
      ioReadBytes: getNumber(arrow, 'io_read_bytes', i),
      ioWriteBytes: getNumber(arrow, 'io_write_bytes', i),
      networkRxBytes: getNumber(arrow, 'network_rx_bytes', i),
      networkTxBytes: getNumber(arrow, 'network_tx_bytes', i),
      threadCount: getNumber(arrow, 'thread_count', i),
      hasCpu: getBool(arrow, 'has_cpu', i),
      hasMemory: getBool(arrow, 'has_memory', i),
      hasIo: getBool(arrow, 'has_io', i),
      hasNetwork: getBool(arrow, 'has_network', i),
    });
  }
  return rows;
}

export function parseThreadTable(bytes: ArrayBuffer | Uint8Array): ThreadRow[] {
  const arrow = arrowBytesToTable(bytes);
  const rows: ThreadRow[] = [];
  for (let i = 0; i < arrow.numRows; i++) {
    rows.push({
      tid: getNumber(arrow, 'tid', i),
      name: getString(arrow, 'name', i),
      cpuPercent: getNumber(arrow, 'cpu_percent', i),
      state: getString(arrow, 'state', i) as ThreadRow['state'],
      stackDepth: getNumber(arrow, 'stack_depth', i),
      waitReason: getString(arrow, 'wait_reason', i) || undefined,
      hasCpu: getBool(arrow, 'has_cpu', i),
    });
  }
  return rows;
}

export function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${(bytes / Math.pow(k, i)).toFixed(1)} ${sizes[i]}`;
}

export function formatPercent(value: number): string {
  return `${value.toFixed(1)}%`;
}
