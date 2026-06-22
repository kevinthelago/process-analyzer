import React, { useCallback, useEffect, useRef, useState } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { invoke } from '@tauri-apps/api/core';
import type { SymbolizedFrame } from '@/contracts/query-engine';
import { useSelectionStore } from '@/store/selection';
import { cn } from '@/lib/utils';

const FRAME_HEIGHT = 56;

interface FrameRowProps {
  frame: SymbolizedFrame;
  index: number;
  isSelected: boolean;
  onClick: () => void;
}

function FrameRow({ frame, index, isSelected, onClick }: FrameRowProps) {
  const isUnresolved = !frame.file;
  return (
    <div
      className={cn(
        'px-3 py-2 border-b border-border/30 cursor-pointer transition-colors',
        isSelected ? 'bg-primary/10' : 'hover:bg-muted/40',
        frame.inlined && 'opacity-70',
      )}
      style={{ minHeight: FRAME_HEIGHT }}
      onClick={onClick}
    >
      <div className="flex items-start gap-2">
        <span className="text-xs text-muted-foreground tabular-nums w-6 pt-0.5 shrink-0">
          {index}
        </span>
        <div className="min-w-0">
          <div
            className={cn(
              'text-sm font-mono truncate',
              isUnresolved && 'text-muted-foreground',
            )}
            title={frame.symbol}
          >
            {frame.symbol}
            {frame.inlined && (
              <span className="ml-1.5 text-xs text-muted-foreground font-sans">[inlined]</span>
            )}
          </div>
          {frame.file && (
            <div
              className="text-xs text-muted-foreground font-mono truncate mt-0.5"
              title={`${frame.file}:${frame.line ?? '?'}`}
            >
              {frame.file}
              {frame.line != null && (
                <span className="text-primary/80">:{frame.line}</span>
              )}
              {frame.column != null && (
                <span className="text-primary/50">:{frame.column}</span>
              )}
            </div>
          )}
          {frame.module && !frame.file && (
            <div className="text-xs text-muted-foreground font-mono mt-0.5">{frame.module}</div>
          )}
        </div>
      </div>
    </div>
  );
}

interface StackPaneProps {
  pid: number;
  tid: number;
}

export function StackPane({ pid, tid }: StackPaneProps) {
  const [frames, setFrames] = useState<SymbolizedFrame[]>([]);
  const [error, setError] = useState<string | null>(null);

  const selectedFrame = useSelectionStore(s => s.selectedFrame);
  const setSelectedFrame = useSelectionStore(s => s.setSelectedFrame);

  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let cancelled = false;

    const fetch = async () => {
      try {
        const stack = await invoke<SymbolizedFrame[]>('get_thread_stack', { pid, tid });
        if (!cancelled) { setFrames(stack); setError(null); }
      } catch (e) {
        if (!cancelled) setError(String(e));
      }
    };

    fetch();
    const id = setInterval(fetch, 2000);
    return () => { cancelled = true; clearInterval(id); };
  }, [pid, tid]);

  const rowVirtualizer = useVirtualizer({
    count: frames.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => FRAME_HEIGHT,
    overscan: 8,
  });

  // Scroll to the selected frame when it comes from outside
  useEffect(() => {
    if (!selectedFrame) return;
    const idx = frames.findIndex(f => f.address === selectedFrame.address);
    if (idx !== -1) {
      rowVirtualizer.scrollToIndex(idx, { align: 'auto' });
    }
  }, [selectedFrame, frames, rowVirtualizer]);

  const handleFrameClick = useCallback(
    (frame: SymbolizedFrame) => {
      const isSame =
        selectedFrame?.address === frame.address &&
        selectedFrame?.file === frame.file &&
        selectedFrame?.line === frame.line;
      setSelectedFrame(
        isSame
          ? null
          : {
              address: frame.address,
              symbol: frame.symbol,
              file: frame.file,
              line: frame.line,
              column: frame.column,
            },
      );
    },
    [selectedFrame, setSelectedFrame],
  );

  if (error) {
    return (
      <div className="text-destructive text-xs p-2 rounded border border-destructive/30 mx-1 mb-2">
        Stack unavailable: {error}
      </div>
    );
  }

  if (frames.length === 0) {
    return (
      <div className="text-muted-foreground text-xs p-2 text-center">
        No stack frames available
      </div>
    );
  }

  const totalHeight = rowVirtualizer.getTotalSize();
  const virtualItems = rowVirtualizer.getVirtualItems();

  return (
    <div className="mt-2">
      <div className="text-xs font-semibold uppercase tracking-wider text-muted-foreground mb-1 px-1">
        Stack ({frames.length} frames)
      </div>
      <div
        ref={scrollRef}
        className="overflow-auto border rounded-md"
        style={{ maxHeight: 400 }}
      >
        <div style={{ height: totalHeight, position: 'relative' }}>
          {virtualItems.map(vItem => {
            const frame = frames[vItem.index];
            const isSelected =
              selectedFrame != null && selectedFrame.address === frame.address;
            return (
              <div
                key={vItem.index}
                data-index={vItem.index}
                ref={rowVirtualizer.measureElement}
                className="absolute top-0 left-0 w-full"
                style={{ transform: `translateY(${vItem.start}px)` }}
              >
                <FrameRow
                  frame={frame}
                  index={vItem.index}
                  isSelected={isSelected}
                  onClick={() => handleFrameClick(frame)}
                />
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
