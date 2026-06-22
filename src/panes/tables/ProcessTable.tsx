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
import type { ProcessRow } from '@/contracts/query-engine';
import { useSelectionStore } from '@/store/selection';
import { usePresetStore } from '@/store/preset';
import { parseProcessTable, formatBytes, formatPercent } from './arrow-utils';
import { cn } from '@/lib/utils';

const col = createColumnHelper<ProcessRow>();

const NA_CELL = <span className="text-muted-foreground">N/A</span>;

const columns = [
  col.accessor('name', {
    header: 'Process',
    size: 200,
    cell: info => (
      <span className="font-medium truncate block max-w-[200px]" title={info.getValue()}>
        {info.getValue()}
      </span>
    ),
  }),
  col.accessor('pid', {
    header: 'PID',
    size: 72,
    cell: info => <span className="tabular-nums">{info.getValue()}</span>,
  }),
  col.accessor('cpuPercent', {
    header: 'CPU %',
    size: 80,
    cell: info =>
      info.row.original.hasCpu ? (
        <CpuBar value={info.getValue()} />
      ) : NA_CELL,
  }),
  col.accessor('memoryBytes', {
    header: 'Memory',
    size: 90,
    cell: info =>
      info.row.original.hasMemory ? formatBytes(info.getValue()) : NA_CELL,
  }),
  col.accessor('ioReadBytes', {
    header: 'I/O Read',
    size: 90,
    cell: info =>
      info.row.original.hasIo ? formatBytes(info.getValue()) : NA_CELL,
  }),
  col.accessor('ioWriteBytes', {
    header: 'I/O Write',
    size: 90,
    cell: info =>
      info.row.original.hasIo ? formatBytes(info.getValue()) : NA_CELL,
  }),
  col.accessor('networkRxBytes', {
    header: 'Net RX',
    size: 90,
    cell: info =>
      info.row.original.hasNetwork ? formatBytes(info.getValue()) : NA_CELL,
  }),
  col.accessor('networkTxBytes', {
    header: 'Net TX',
    size: 90,
    cell: info =>
      info.row.original.hasNetwork ? formatBytes(info.getValue()) : NA_CELL,
  }),
  col.accessor('threadCount', {
    header: 'Threads',
    size: 72,
    cell: info => <span className="tabular-nums">{info.getValue()}</span>,
  }),
];

function CpuBar({ value }: { value: number }) {
  const clamped = Math.min(100, Math.max(0, value));
  const color =
    clamped > 80 ? 'bg-red-500' : clamped > 50 ? 'bg-amber-400' : 'bg-emerald-500';
  return (
    <div className="flex items-center gap-1.5">
      <div className="flex-1 h-1.5 bg-muted rounded-full overflow-hidden">
        <div className={cn('h-full rounded-full', color)} style={{ width: `${clamped}%` }} />
      </div>
      <span className="tabular-nums text-xs w-10 text-right">{formatPercent(value)}</span>
    </div>
  );
}

const ROW_HEIGHT = 36;
const HEADER_HEIGHT = 36;
const FETCH_INTERVAL_MS = 1000;

export function ProcessTable() {
  const [rows, setRows] = useState<ProcessRow[]>([]);
  const [sorting, setSorting] = useState<SortingState>([
    { id: 'cpuPercent', desc: true },
  ]);
  const [error, setError] = useState<string | null>(null);

  const selectedPid = useSelectionStore(s => s.selectedPid);
  const setSelectedPid = useSelectionStore(s => s.setSelectedPid);
  const setSelectedTid = useSelectionStore(s => s.setSelectedTid);

  const activePreset = usePresetStore(s => s.activePreset);

  // When a preset is applied, adopt its sort; on revert, restore manual sort.
  useEffect(() => {
    if (activePreset?.processSort) {
      setSorting(
        activePreset.processSort.map(s => ({ id: s.column as string, desc: s.desc })),
      );
    }
  }, [activePreset]);

  const scrollRef = useRef<HTMLDivElement>(null);

  const fetchData = useCallback(async () => {
    try {
      const bytes = await invoke<number[]>('get_process_aggregates');
      setRows(parseProcessTable(new Uint8Array(bytes)));
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    fetchData();
    const id = setInterval(fetchData, FETCH_INTERVAL_MS);
    return () => clearInterval(id);
  }, [fetchData]);

  const filteredRows = activePreset?.processFilter
    ? rows.filter(activePreset.processFilter)
    : rows;

  const table = useReactTable({
    data: filteredRows,
    columns,
    state: { sorting },
    onSortingChange: setSorting,
    getCoreRowModel: getCoreRowModel(),
    getSortedRowModel: getSortedRowModel(),
    getRowId: row => String(row.pid),
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
    if (selectedPid == null) return;
    const idx = tableRows.findIndex(r => r.original.pid === selectedPid);
    if (idx !== -1) {
      rowVirtualizer.scrollToIndex(idx, { align: 'auto' });
    }
  }, [selectedPid, tableRows, rowVirtualizer]);

  const handleRowClick = useCallback(
    (row: Row<ProcessRow>) => {
      const pid = row.original.pid;
      setSelectedPid(pid === selectedPid ? null : pid);
      setSelectedTid(null); // clear thread selection when process changes
    },
    [selectedPid, setSelectedPid, setSelectedTid],
  );

  const totalHeight = rowVirtualizer.getTotalSize();
  const virtualItems = rowVirtualizer.getVirtualItems();

  if (error) {
    return (
      <div className="flex items-center justify-center h-full text-destructive text-sm p-4">
        Failed to load processes: {error}
      </div>
    );
  }

  return (
    <div className="flex flex-col h-full select-none">
      {/* Header */}
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

      {/* Virtualized body */}
      <div ref={scrollRef} className="flex-1 overflow-auto">
        <div style={{ height: totalHeight, position: 'relative' }}>
          {virtualItems.map(vRow => {
            const row = tableRows[vRow.index];
            const isSelected = row.original.pid === selectedPid;
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
        {rows.length} processes
      </div>
    </div>
  );
}
