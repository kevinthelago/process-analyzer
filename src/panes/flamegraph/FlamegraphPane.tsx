import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { PixiRenderer } from '../../render/PixiRenderer';
import { Canvas2dFallback } from '../../render/Canvas2dFallback';
import type { Viewport } from '../../render/types';
import type { TimeRange } from '../../contracts/selection-store';
import { buildFlamegraphFrame } from './FlamegraphRenderer';
import { useFlamegraph } from './useFlamegraph';
import { SearchOverlay } from './SearchOverlay';

interface FlamegraphPaneProps {
  pid: number | null;
  tid: number | null;
  timeRange: TimeRange | null;
  /** Called when user clicks a frame (to focus the selection store) */
  onFrameFocus: (frame: string | null) => void;
  className?: string;
}

/**
 * Self-sizing flame graph pane.
 *
 * - Click a frame to zoom into it (double-click root to reset).
 * - Ctrl+F to search frames (matches highlighted, others dimmed).
 * - Canvas scrolls vertically when content exceeds container height.
 */
export const FlamegraphPane: React.FC<FlamegraphPaneProps> = ({
  pid,
  tid,
  timeRange,
  onFrameFocus,
  className,
}) => {
  const containerRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const rendererRef = useRef<PixiRenderer | Canvas2dFallback | null>(null);
  const rafRef = useRef<number>(0);
  const [showSearch, setShowSearch] = useState(false);

  const { width } = useContainerSize(containerRef);
  // Fixed height based on content; canvas scrolls inside container
  const canvasHeight = 600;

  const [{ rects, zoomedFrameId, searchQuery, isLoading }, { handleClick, setSearch, resetZoom }] =
    useFlamegraph({ pid, tid, timeRange, width });

  const matchCount = useMemo(
    () => (searchQuery ? rects.filter((r) => r.highlighted).length : 0),
    [rects, searchQuery],
  );

  // Static viewport: flame graph coordinates ARE screen coordinates (no world transform)
  const viewport: Viewport = useMemo(
    () => ({ tpp: 1, vpp: 1, left: 0, top: 0, width, height: canvasHeight }),
    [width, canvasHeight],
  );

  // Init renderer
  useEffect(() => {
    if (!canvasRef.current) return;
    try {
      rendererRef.current = new PixiRenderer(canvasRef.current, { backgroundColor: 0x0d1117 });
    } catch {
      rendererRef.current = new Canvas2dFallback(canvasRef.current!, { backgroundColor: 0x0d1117 });
    }
    return () => {
      rendererRef.current?.destroy();
      rendererRef.current = null;
    };
  }, []);

  useEffect(() => {
    rendererRef.current?.resize(width, canvasHeight);
  }, [width, canvasHeight]);

  // Render loop
  useEffect(() => {
    function frame() {
      if (!rendererRef.current) return;
      const sceneFrame = buildFlamegraphFrame(rects, zoomedFrameId, searchQuery.length > 0);
      rendererRef.current.render(sceneFrame, viewport);
      rafRef.current = requestAnimationFrame(frame);
    }
    rafRef.current = requestAnimationFrame(frame);
    return () => cancelAnimationFrame(rafRef.current);
  }, [rects, zoomedFrameId, searchQuery, viewport]);

  // Click handling
  const onClick = useCallback(
    (e: React.MouseEvent<HTMLCanvasElement>) => {
      if (!rendererRef.current) return;
      const rect = e.currentTarget.getBoundingClientRect();
      const sx = e.clientX - rect.left;
      const sy = e.clientY - rect.top;
      const hit = rendererRef.current.hitTest(viewport, sx, sy);
      if (!hit) {
        resetZoom();
        onFrameFocus(null);
        return;
      }
      handleClick(hit.id);
      const fg = rects.find((r) => r.nodeId === hit.id);
      onFrameFocus(fg?.frame ?? null);
    },
    [viewport, rects, handleClick, resetZoom, onFrameFocus],
  );

  // Keyboard shortcut for search
  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      if ((e.ctrlKey || e.metaKey) && e.key === 'f') {
        e.preventDefault();
        setShowSearch(true);
      }
    }
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, []);

  return (
    <div ref={containerRef} className={`relative overflow-auto bg-[#0d1117] ${className ?? ''}`}>
      {isLoading && (
        <div className="absolute inset-0 flex items-center justify-center text-[#8b949e] text-sm z-10">
          Loading…
        </div>
      )}
      {showSearch && (
        <SearchOverlay
          value={searchQuery}
          matchCount={matchCount}
          onChange={setSearch}
          onClose={() => setShowSearch(false)}
        />
      )}
      <canvas
        ref={canvasRef}
        width={width}
        height={canvasHeight}
        style={{ display: 'block', cursor: 'pointer' }}
        onClick={onClick}
        onDoubleClick={resetZoom}
      />
    </div>
  );
};

function useContainerSize(ref: React.RefObject<HTMLDivElement>): { width: number; height: number } {
  const [size, setSize] = React.useState({ width: 800, height: 600 });

  useEffect(() => {
    if (!ref.current) return;
    const ro = new ResizeObserver(([entry]) => {
      const { width, height } = entry.contentRect;
      setSize({ width: Math.max(1, Math.round(width)), height: Math.max(1, Math.round(height)) });
    });
    ro.observe(ref.current);
    return () => ro.disconnect();
  }, [ref]);

  return size;
}
