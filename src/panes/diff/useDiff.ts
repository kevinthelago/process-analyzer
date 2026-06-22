import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { tableFromIPC } from 'apache-arrow';
import { buildDiffTree, type RawDiffRow } from './DiffFlamegraphRenderer';
import type { DiffNode, DiffTableRow } from './types';
import { DELTA_THRESHOLD } from './types';

interface UseDiffOptions {
  baselineTraceId: string | null;
  regressionTraceId: string | null;
  pid: number | null;
  tid: number | null;
  timeRange: [number, number] | null;
}

interface DiffState {
  root: DiffNode | null;
  tableRows: DiffTableRow[];
  isLoading: boolean;
  /** Which side is currently the "base" (supports baseline swap) */
  swapped: boolean;
}

interface DiffActions {
  swapBaseline: () => void;
  refresh: () => void;
}

export function useDiff(opts: UseDiffOptions): [DiffState, DiffActions] {
  const [root, setRoot] = useState<DiffNode | null>(null);
  const [tableRows, setTableRows] = useState<DiffTableRow[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [swapped, setSwapped] = useState(false);
  const [version, setVersion] = useState(0);

  const baseId = swapped ? opts.regressionTraceId : opts.baselineTraceId;
  const regId = swapped ? opts.baselineTraceId : opts.regressionTraceId;

  useEffect(() => {
    if (!baseId || !regId) return;
    let cancelled = false;
    setIsLoading(true);

    invoke<ArrayBuffer>('query_call_tree_diff', {
      baselineTraceId: baseId,
      regressionTraceId: regId,
      pid: opts.pid ?? null,
      tid: opts.tid ?? null,
      timeRangeNs: opts.timeRange ?? null,
    })
      .then((buf) => {
        if (cancelled) return;
        const table = tableFromIPC(new Uint8Array(buf));
        const rows = arrowToDiffRows(table);
        const tree = buildDiffTree(rows);
        setRoot(tree);
        setTableRows(buildTableRows(rows));
      })
      .catch(console.error)
      .finally(() => {
        if (!cancelled) setIsLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, [baseId, regId, opts.pid, opts.tid, opts.timeRange, version]);

  const swapBaseline = useCallback(() => setSwapped((s) => !s), []);
  const refresh = useCallback(() => setVersion((v) => v + 1), []);

  return [{ root, tableRows, isLoading, swapped }, { swapBaseline, refresh }];
}

function arrowToDiffRows(table: ReturnType<typeof tableFromIPC>): RawDiffRow[] {
  const rows: RawDiffRow[] = [];
  for (let i = 0; i < table.numRows; i++) {
    rows.push({
      id: Number(table.getChildAt(0)?.get(i) ?? i),
      parentId: Number(table.getChildAt(1)?.get(i) ?? -1),
      frame: String(table.getChildAt(2)?.get(i) ?? ''),
      baselineNs: Number(table.getChildAt(3)?.get(i) ?? 0),
      regressionNs: Number(table.getChildAt(4)?.get(i) ?? 0),
      deltaPct: Number(table.getChildAt(5)?.get(i) ?? 0),
      depth: Number(table.getChildAt(6)?.get(i) ?? 0),
    });
  }
  return rows;
}

function buildTableRows(rows: RawDiffRow[]): DiffTableRow[] {
  return rows
    .map((r) => ({
      frame: r.frame,
      baselineNs: r.baselineNs,
      regressionNs: r.regressionNs,
      deltaPct: r.deltaPct,
      deltaAbsNs: r.regressionNs - r.baselineNs,
    }))
    .filter((r) => Math.abs(r.deltaPct) > DELTA_THRESHOLD)
    .sort((a, b) => Math.abs(b.deltaPct) - Math.abs(a.deltaPct))
    .slice(0, 50);
}
