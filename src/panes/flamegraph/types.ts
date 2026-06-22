/** A node in the call tree (from Arrow IPC) */
export interface CallNode {
  id: number;
  parentId: number;
  frame: string;
  /** Exclusive (self) time in nanoseconds */
  selfNs: number;
  /** Inclusive (total) time in nanoseconds */
  totalNs: number;
  depth: number;
  children: CallNode[];
}

/** Laid-out rectangle for a CallNode, ready to render */
export interface FlamegraphRect {
  nodeId: number;
  frame: string;
  selfNs: number;
  totalNs: number;
  /** Screen-space x */
  x: number;
  /** Screen-space y (from top) */
  y: number;
  w: number;
  h: number;
  color: number;
  /** Whether this frame matches the current search query */
  highlighted: boolean;
}

export const FRAME_HEIGHT = 20;

/** Deterministic color from frame name (stable across renders) */
export function frameColor(frame: string): number {
  let hash = 0;
  for (let i = 0; i < frame.length; i++) {
    hash = (Math.imul(31, hash) + frame.charCodeAt(i)) | 0;
  }
  const h = Math.abs(hash) % 360;
  // Convert HSL (h, 65%, 50%) → RGB
  return hslToRgb(h, 65, 50);
}

function hslToRgb(h: number, s: number, l: number): number {
  s /= 100;
  l /= 100;
  const a = s * Math.min(l, 1 - l);
  const f = (n: number) => {
    const k = (n + h / 30) % 12;
    return l - a * Math.max(-1, Math.min(k - 3, 9 - k, 1));
  };
  const r = Math.round(f(0) * 255);
  const g = Math.round(f(8) * 255);
  const b = Math.round(f(4) * 255);
  return (r << 16) | (g << 8) | b;
}
