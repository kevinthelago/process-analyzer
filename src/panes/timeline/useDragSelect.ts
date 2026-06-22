import { useCallback, useRef, useState } from 'react';
import type { Viewport } from '../../render/types';
import { screenXToWorld } from '../../render/viewport';

export interface DragState {
  /** Active drag range in world-space ns, null when not dragging */
  range: [number, number] | null;
  isDragging: boolean;
}

export interface DragSelectHandlers {
  onPointerDown: (e: React.PointerEvent<HTMLCanvasElement>) => void;
  onPointerMove: (e: React.PointerEvent<HTMLCanvasElement>) => void;
  onPointerUp: (e: React.PointerEvent<HTMLCanvasElement>) => void;
}

/**
 * Drag-to-select hook for the timeline canvas.
 *
 * Returns the current drag range (world-space ns) and pointer event handlers.
 * On drag commit (pointerup) it calls onCommit with the selected range.
 */
export function useDragSelect(
  getViewport: () => Viewport,
  onCommit: (range: [number, number] | null) => void,
): [DragState, DragSelectHandlers] {
  const startX = useRef<number | null>(null);
  const [range, setRange] = useState<[number, number] | null>(null);
  const [isDragging, setIsDragging] = useState(false);

  const onPointerDown = useCallback(
    (e: React.PointerEvent<HTMLCanvasElement>) => {
      if (e.button !== 0) return;
      e.currentTarget.setPointerCapture(e.pointerId);
      const vp = getViewport();
      const rect = e.currentTarget.getBoundingClientRect();
      const sx = e.clientX - rect.left;
      startX.current = screenXToWorld(vp, sx);
      setIsDragging(true);
      setRange(null);
    },
    [getViewport],
  );

  const onPointerMove = useCallback(
    (e: React.PointerEvent<HTMLCanvasElement>) => {
      if (startX.current === null) return;
      const vp = getViewport();
      const rect = e.currentTarget.getBoundingClientRect();
      const sx = e.clientX - rect.left;
      const worldX = screenXToWorld(vp, sx);
      const lo = Math.min(startX.current, worldX);
      const hi = Math.max(startX.current, worldX);
      setRange([lo, hi]);
    },
    [getViewport],
  );

  const onPointerUp = useCallback(
    (e: React.PointerEvent<HTMLCanvasElement>) => {
      if (startX.current === null) return;
      const vp = getViewport();
      const rect = e.currentTarget.getBoundingClientRect();
      const sx = e.clientX - rect.left;
      const worldX = screenXToWorld(vp, sx);
      const lo = Math.min(startX.current, worldX);
      const hi = Math.max(startX.current, worldX);

      startX.current = null;
      setIsDragging(false);

      // Ignore tiny accidental clicks (< 5ns span)
      if (hi - lo < 5) {
        setRange(null);
        onCommit(null);
        return;
      }

      const committed: [number, number] = [lo, hi];
      setRange(committed);
      onCommit(committed);
    },
    [getViewport, onCommit],
  );

  return [{ range, isDragging }, { onPointerDown, onPointerMove, onPointerUp }];
}
