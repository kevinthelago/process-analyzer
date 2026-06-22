import type { DrawRect, DrawText, SceneFrame } from '../../render/types';
import { formatNs } from '../../render/scales';
import type { FlamegraphRect } from './types';

const HIGHLIGHT_BORDER = 0xf0c040;
const DIMMED_ALPHA = 0.3;

/**
 * Converts laid-out FlamegraphRects into a render-engine SceneFrame.
 *
 * When a search query is active, non-matching frames are dimmed.
 * The currently focused frame (zoomedFrame) gets a bright border outline.
 */
export function buildFlamegraphFrame(
  rects: FlamegraphRect[],
  zoomedFrameId: number | null,
  hasSearch: boolean,
): SceneFrame {
  const drawRects: DrawRect[] = [];
  const texts: DrawText[] = [];

  for (const r of rects) {
    const alpha = hasSearch && !r.highlighted ? DIMMED_ALPHA : 0.95;
    drawRects.push({
      id: r.nodeId,
      x: r.x,
      y: r.y,
      w: r.w,
      h: r.h,
      color: r.color,
      alpha,
    });

    // Highlight border for zoomed frame
    if (r.nodeId === zoomedFrameId) {
      drawRects.push({ id: 0, x: r.x, y: r.y, w: r.w, h: 1, color: HIGHLIGHT_BORDER, alpha: 1 });
      drawRects.push({ id: 0, x: r.x, y: r.y + r.h - 1, w: r.w, h: 1, color: HIGHLIGHT_BORDER, alpha: 1 });
      drawRects.push({ id: 0, x: r.x, y: r.y, w: 1, h: r.h, color: HIGHLIGHT_BORDER, alpha: 1 });
      drawRects.push({ id: 0, x: r.x + r.w - 1, y: r.y, w: 1, h: r.h, color: HIGHLIGHT_BORDER, alpha: 1 });
    }

    if (r.w >= 40) {
      const label = r.w >= 120 ? `${r.frame} (${formatNs(r.totalNs)})` : r.frame;
      texts.push({
        x: r.x,
        y: r.y,
        text: label,
        color: 0xffffff,
        fontSize: 10,
        minPx: r.w,
      });
    }
  }

  return { rects: drawRects, lines: [], texts };
}
