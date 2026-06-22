import React, { useCallback, useEffect, useRef } from 'react';
import { PixiRenderer } from '../../render/PixiRenderer';
import { Canvas2dFallback } from '../../render/Canvas2dFallback';
import type { Viewport } from '../../render/types';
import type { TimeRange } from '../../contracts/selection-store';
import { buildTimelineFrame } from './TimelineRenderer';
import { useDragSelect } from './useDragSelect';
import { useTimeline } from './useTimeline';

interface TimelinePaneProps {
  pid: number | null;
  tid: number | null;
  /** Wired to app-shell setTimeRange from the Zustand selection store */
  onTimeRangeChange: (range: TimeRange | null) => void;
  selectedTimeRange: TimeRange | null;
  className?: string;
}

/**
 * Multi-track timeline pane.
 *
 * - Renders CPU/IO/syscall events as colored rects per thread track.
 * - Drag to select a time range (committed to selection store).
 * - Scroll-wheel zooms horizontally around the cursor.
 * - Two-finger pan (or middle-mouse drag) scrolls.
 */
export const TimelinePane: React.FC<TimelinePaneProps> = ({
  pid,
  tid,
  onTimeRangeChange,
  selectedTimeRange,
  className,
}) => {
  const containerRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const rendererRef = useRef<PixiRenderer | Canvas2dFallback | null>(null);
  const rafRef = useRef<number>(0);
  const isPanning = useRef(false);
  const lastPanX = useRef(0);

  const { width, height } = useContainerSize(containerRef);

  const [{ tracks, viewport, isLoading }, { panBy, zoomAt, resetZoom }] = useTimeline({
    pid,
    tid,
    width,
    height,
  });

  const vpRef = useRef<Viewport>(viewport);
  vpRef.current = viewport;

  const getViewport = useCallback(() => vpRef.current, []);

  const [dragState, dragHandlers] = useDragSelect(getViewport, onTimeRangeChange);

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

  // Resize renderer
  useEffect(() => {
    rendererRef.current?.resize(width, height);
  }, [width, height]);

  // Render loop
  useEffect(() => {
    function frame() {
      if (!rendererRef.current) return;
      const selRange = dragState.range ?? selectedTimeRange;
      const sceneFrame = buildTimelineFrame(tracks, vpRef.current, selRange);
      rendererRef.current.render(sceneFrame, vpRef.current);
      rafRef.current = requestAnimationFrame(frame);
    }
    rafRef.current = requestAnimationFrame(frame);
    return () => cancelAnimationFrame(rafRef.current);
  }, [tracks, dragState.range, selectedTimeRange]);

  // Wheel zoom
  const onWheel = useCallback(
    (e: React.WheelEvent<HTMLCanvasElement>) => {
      e.preventDefault();
      const rect = e.currentTarget.getBoundingClientRect();
      const sx = e.clientX - rect.left;
      const factor = e.deltaY < 0 ? 1.12 : 1 / 1.12;
      zoomAt(sx, factor);
    },
    [zoomAt],
  );

  // Middle-mouse pan
  const onMouseDown = useCallback((e: React.MouseEvent<HTMLCanvasElement>) => {
    if (e.button === 1) {
      isPanning.current = true;
      lastPanX.current = e.clientX;
    }
  }, []);

  const onMouseMove = useCallback(
    (e: React.MouseEvent<HTMLCanvasElement>) => {
      if (!isPanning.current) return;
      panBy(lastPanX.current - e.clientX);
      lastPanX.current = e.clientX;
    },
    [panBy],
  );

  const onMouseUp = useCallback(() => {
    isPanning.current = false;
  }, []);

  // Double-click resets zoom
  const onDoubleClick = useCallback(() => {
    resetZoom();
    onTimeRangeChange(null);
  }, [resetZoom, onTimeRangeChange]);

  return (
    <div ref={containerRef} className={`relative overflow-hidden bg-[#0d1117] ${className ?? ''}`}>
      {isLoading && (
        <div className="absolute inset-0 flex items-center justify-center text-[#8b949e] text-sm z-10">
          Loading…
        </div>
      )}
      <canvas
        ref={canvasRef}
        width={width}
        height={height}
        style={{ display: 'block', cursor: dragState.isDragging ? 'col-resize' : 'crosshair' }}
        onWheel={onWheel}
        onMouseDown={onMouseDown}
        onMouseMove={onMouseMove}
        onMouseUp={onMouseUp}
        onDoubleClick={onDoubleClick}
        {...dragHandlers}
      />
    </div>
  );
};

// ---------------------------------------------------------------------------
// Utility hook
// ---------------------------------------------------------------------------

function useContainerSize(ref: React.RefObject<HTMLDivElement>): { width: number; height: number } {
  const [size, setSize] = React.useState({ width: 800, height: 400 });

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
