import React, { useCallback, useEffect, useRef, useState } from 'react';
import {
  createColumnHelper,
  flexRender,
  getCoreRowModel,
  getSortedRowModel,
  SortingState,
  useReactTable,
  Row,
} from '@tanstack/react-table';
import { useVirtualizer } from '@tanstack/react-virtual';
import { invoke } from '@tauri-apps/api/core';
import type { ThreadRow } from '@/contracts/query-engine';
import { useSelectionStore } from '@/store/selection';
import { usePresetStore } from '@/store/preset';
import { parseThreadTable, formatPercent } from './arrow-utils';
import { cn } from '@/lib/utils';

const col = createColumnHelper<ThreadRow>();

const THREAD_STATE_COLORS: Record<string, string> = {
  running: 'bg-emerald-500',
  sleeping: 'bg-sky-400',
  blocked: 'bg-red-500',
  idle: 'bg-muted-foreground/40',
  zombie: 'bg-rose-800',
  unknown: 'bg-muted-foreground/20',
};

function StateBadge({ state }: { state: ThreadRow['state'] }) {
  return (
    <span className="flex items-center gap-1.5">
      <span className={cn('inline-block h-1.5 w-1.5 rounded-full', THREAD_STATE_COLORS[state])} />
      <span className="text-xs capitalize">{state}</span>
    </span>
  );
}

const columns = [
  col.accessor('name', {
    header: 'Thread',
    size: 200,
    cell: info => (
      <span className="truncate block max-w-[200px]" title={info.getValue()}>
        {info.getValue() || `Thread ${info.row.original.tid}`}
      </span>
    ),
  }),
  col.accessor('tid', {
    header: 'TID',
    size: 72,
    cell: info => <span className="tabular-nums">{info.getValue()}</span>,
  }),
  col.accessor('cpuPercent', {
    header: 'CPU %',
    size: 90,
    cell: info =>
      info.row.original.hasCpu ? (
        <span className="tabular-nums">{formatPercent(info.getValue())}</span>
      ) : (
        <span className="text-muted-foreground">N/A</span>
      ),
  }),
  col.accessor('state', {
    header: 'State',
    size: 100,
    cell: info => <StateBadge state={info.getValue()} />,
  }),
  col.accessor('stackDepth', {
    header: 'Stack',
    size: 60,
    cell: info => <span className="tabular-nums">{info.getValue()}</span>,
  }),
  col.accessor('waitReason', {
    header: 'Wait Reason',
    size: 180,
    cell: info => (
      <span className="text-muted-foreground truncate block" title={info.getValue()}>
        {info.getValue() ?? '—'}
      </span>
    ),
  }),
];

const ROW_HEIGHT = 34;
const HEADER_HEIGHT = 36;
const FETCH_INTERVAL_MS = 1000;

export function ThreadTable() {
  const [rows, setRows] = useState<ThreadRow[]>([]);
  const [sorting, setSorting] = useState<SortingState>([
    { id: 'cpuPercent', desc: true },
  ]);
  const [error, setError] = useState<string | null>(null);

  const selectedPid = useSelectionStore(s => s.selectedPid);
  const selectedTid = useSelectionStore(s => s.selectedTid);
  const setSelectedTid = useSelectionStore(s => s.setSelectedTid);

  const activePreset = usePresetStore(s => s.activePreset);

  useEffect(() => {
    if (activePreset?.threadSort) {
      setSorting(
        activePreset.threadSort.map(s => ({ id: s.column as string, desc: s.desc })),
      );
    }
  }, [activePreset]);

  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (selectedPid == null) {
      setRows([]);
      return;
    }

    let cancelled = false;

    const fetch = async () => {
      try {
        const bytes = await invoke<number[]>('get_thread_aggregates', { pid: selectedPid });
        if (!cancelled) {
          setRows(parseThreadTable(new Uint8Array(bytes)));
          setError(null);
        }
      } catch (e) {
        if (!cancelled) setError(String(e));
      }
    };

    fetch();
    const id = setInterval(fetch, FETCH_INTERVAL_MS);
    return () => {
      cancelled = true;
      clearInterval(id);
    };
  }, [selectedPid]);

  const filteredRows = activePreset?.threadFilter
    ? rows.filter(activePreset.threadFilter)
    : rows;

  const table = useReactTable({
    data: filteredRows,
    columns,
    state: { sorting },
    onSortingChange: setSorting,
    getCoreRowModel: getCoreRowModel(),
    getSortedRowModel: getSortedRowModel(),
    getRowId: row => String(row.tid),
  });

  const { rows: tableRows } = table.getRowModel();

  const rowVirtualizer = useVirtualizer({
    count: tableRows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 12,
  });

  // Scroll to highlighted row when selection comes from outside
  useEffect(() => {
    if (selectedTid == null) return;
    const idx = tableRows.findIndex(r => r.original.tid === selectedTid);
    if (idx !== -1) {
      rowVirtualizer.scrollToIndex(idx, { align: 'auto' });
    }
  }, [selectedTid, tableRows, rowVirtualizer]);

  const handleRowClick = useCallback(
    (row: Row<ThreadRow>) => {
      const tid = row.original.tid;
      setSelectedTid(tid === selectedTid ? null : tid);
    },
    [selectedTid, setSelectedTid],
  );

  const totalHeight = rowVirtualizer.getTotalSize();
  const virtualItems = rowVirtualizer.getVirtualItems();

  if (selectedPid == null) {
    return (
      <div className="flex items-center justify-center h-full text-sm text-muted-foreground">
        Select a process to view threads
      </div>
    );
  }

  if (error) {
    return (
      <div className="flex items-center justify-center h-full text-destructive text-sm p-4">
        Failed to load threads: {error}
      </div>
    );
  }

  return (
    <div className="flex flex-col h-full select-none">
      <div
        className="flex shrink-0 border-b bg-muted/40 text-xs font-medium text-muted-foreground"
        style={{ height: HEADER_HEIGHT }}
      >
        {table.getHeaderGroups().map(hg =>
          hg.headers.map(header => (
            <div
              key={header.id}
              className={cn(
                'flex items-center px-2 gap-1 cursor-pointer select-none hover:text-foreground transition-colors',
                header.column.getCanSort() && 'hover:bg-muted/60',
              )}
              style={{ width: header.getSize() }}
              onClick={header.column.getToggleSortingHandler()}
            >
              {flexRender(header.column.columnDef.header, header.getContext())}
              {header.column.getIsSorted() === 'asc' && <span>↑</span>}
              {header.column.getIsSorted() === 'desc' && <span>↓</span>}
            </div>
          )),
        )}
      </div>

      <div ref={scrollRef} className="flex-1 overflow-auto">
        <div style={{ height: totalHeight, position: 'relative' }}>
          {virtualItems.map(vRow => {
            const row = tableRows[vRow.index];
            const isSelected = row.original.tid === selectedTid;
            return (
              <div
                key={row.id}
                data-index={vRow.index}
                ref={rowVirtualizer.measureElement}
                className={cn(
                  'absolute top-0 left-0 w-full flex items-center text-sm border-b border-border/40 cursor-pointer transition-colors',
                  isSelected
                    ? 'bg-primary/10 hover:bg-primary/15 font-medium'
                    : 'hover:bg-muted/40',
                )}
                style={{ transform: `translateY(${vRow.start}px)`, height: ROW_HEIGHT }}
                onClick={() => handleRowClick(row)}
              >
                {row.getVisibleCells().map(cell => (
                  <div
                    key={cell.id}
                    className="px-2 overflow-hidden text-ellipsis whitespace-nowrap"
                    style={{ width: cell.column.getSize() }}
                  >
                    {flexRender(cell.column.columnDef.cell, cell.getContext())}
                  </div>
                ))}
              </div>
            );
          })}
        </div>
      </div>

      <div className="shrink-0 border-t px-3 py-1 text-xs text-muted-foreground">
        {rows.length} threads
      </div>
    </div>
  );
}
