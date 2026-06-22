import React, { useCallback, useEffect, useMemo, useRef } from 'react';
import { PixiRenderer } from '../../render/PixiRenderer';
import { Canvas2dFallback } from '../../render/Canvas2dFallback';
import type { Viewport } from '../../render/types';
import { buildDiffFrame, layoutDiffFlamegraph } from './DiffFlamegraphRenderer';
import { useDiff } from './useDiff';
import { DiffTable } from './DiffTable';

interface DiffPaneProps {
  baselineTraceId: string | null;
  regressionTraceId: string | null;
  pid: number | null;
  tid: number | null;
  timeRange: [number, number] | null;
  className?: string;
}

/**
 * Baseline-vs-regression diff pane.
 *
 * Shows:
 *   1. A diff flame graph (red = regressed, green = improved, gray = neutral).
 *   2. A tabular summary of the top Δ frames.
 *   3. A "Swap baseline" button to flip the comparison direction.
 */
export const DiffPane: React.FC<DiffPaneProps> = ({
  baselineTraceId,
  regressionTraceId,
  pid,
  tid,
  timeRange,
  className,
}) => {
  const containerRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const rendererRef = useRef<PixiRenderer | Canvas2dFallback | null>(null);
  const rafRef = useRef<number>(0);

  const { width } = useContainerSize(containerRef);
  const canvasHeight = 400;

  const [{ root, tableRows, isLoading, swapped }, { swapBaseline }] = useDiff({
    baselineTraceId,
    regressionTraceId,
    pid,
    tid,
    timeRange,
  });

  const rects = useMemo(
    () => (root ? layoutDiffFlamegraph(root, width) : []),
    [root, width],
  );

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
      const sceneFrame = buildDiffFrame(rects);
      rendererRef.current.render(sceneFrame, viewport);
      rafRef.current = requestAnimationFrame(frame);
    }
    rafRef.current = requestAnimationFrame(frame);
    return () => cancelAnimationFrame(rafRef.current);
  }, [rects, viewport]);

  const hasData = baselineTraceId && regressionTraceId;

  return (
    <div ref={containerRef} className={`flex flex-col bg-[#0d1117] ${className ?? ''}`}>
      {/* Header */}
      <div className="flex items-center justify-between px-4 py-2 border-b border-[#21262d]">
        <span className="text-[#8b949e] text-xs">
          {swapped ? 'Swapped: regression → baseline' : 'Baseline vs Regression'}
        </span>
        <button
          onClick={swapBaseline}
          disabled={!hasData}
          className="text-xs text-[#58a6ff] hover:underline disabled:text-[#6e7681] disabled:cursor-not-allowed"
        >
          Swap baseline
        </button>
      </div>

      {/* Flame graph */}
      <div className="relative">
        {isLoading && (
          <div className="absolute inset-0 flex items-center justify-center text-[#8b949e] text-sm z-10 bg-[#0d1117]/80">
            Loading diff…
          </div>
        )}
        {!hasData && !isLoading && (
          <div className="flex items-center justify-center h-32 text-[#6e7681] text-sm">
            Select a baseline and regression trace to compare.
          </div>
        )}
        {hasData && (
          <canvas
            ref={canvasRef}
            width={width}
            height={canvasHeight}
            style={{ display: 'block' }}
          />
        )}
      </div>

      {/* Legend */}
      {hasData && !isLoading && (
        <div className="flex items-center gap-4 px-4 py-2 text-xs text-[#8b949e]">
          <span className="flex items-center gap-1">
            <span className="inline-block w-3 h-3 rounded-sm bg-[#e05252]" />
            Slower (&gt;5%)
          </span>
          <span className="flex items-center gap-1">
            <span className="inline-block w-3 h-3 rounded-sm bg-[#3fb950]" />
            Faster (&gt;5%)
          </span>
          <span className="flex items-center gap-1">
            <span className="inline-block w-3 h-3 rounded-sm bg-[#3d444d]" />
            Unchanged
          </span>
        </div>
      )}

      {/* Table */}
      {hasData && !isLoading && tableRows.length > 0 && (
        <div className="border-t border-[#21262d] mt-1">
          <div className="px-4 py-2 text-xs font-semibold text-[#8b949e] uppercase tracking-wider">
            Top Regressions / Improvements
          </div>
          <DiffTable rows={tableRows} />
        </div>
      )}
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
