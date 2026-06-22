import type { DrawRect, DrawText, SceneFrame } from '../../render/types';
import { formatDeltaPct } from '../../render/scales';
import type { DiffNode, DiffRect } from './types';
import { diffColor } from './types';

export const FRAME_HEIGHT = 20;

/**
 * Lay out a DiffNode tree as a flame graph sized by regression time.
 * Colors indicate delta: red = slower, green = faster, gray = neutral.
 */
export function layoutDiffFlamegraph(root: DiffNode, width: number): DiffRect[] {
  const rects: DiffRect[] = [];
  const scale = width / root.regressionNs;

  function visit(node: DiffNode, x: number, depth: number): void {
    const w = node.regressionNs * scale;
    if (w < 1) return;
    const y = depth * FRAME_HEIGHT;

    rects.push({
      nodeId: node.id,
      frame: node.frame,
      baselineNs: node.baselineNs,
      regressionNs: node.regressionNs,
      deltaPct: node.deltaPct,
      x,
      y,
      w,
      h: FRAME_HEIGHT - 1,
      color: diffColor(node.deltaPct),
    });

    let childX = x;
    for (const child of node.children) {
      visit(child, childX, depth + 1);
      childX += child.regressionNs * scale;
    }
  }

  visit(root, 0, 0);
  return rects;
}

export function buildDiffFrame(rects: DiffRect[]): SceneFrame {
  const drawRects: DrawRect[] = [];
  const texts: DrawText[] = [];

  for (const r of rects) {
    drawRects.push({
      id: r.nodeId,
      x: r.x,
      y: r.y,
      w: r.w,
      h: r.h,
      color: r.color,
      alpha: 0.92,
    });

    if (r.w >= 60) {
      const delta = r.deltaPct !== 0 ? ` ${formatDeltaPct(r.deltaPct)}` : '';
      const label = r.w >= 140 ? `${r.frame}${delta}` : r.frame;
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

/** Build a DiffNode tree from a flat list of raw diff rows */
export interface RawDiffRow {
  id: number;
  parentId: number;
  frame: string;
  baselineNs: number;
  regressionNs: number;
  deltaPct: number;
  depth: number;
}

export function buildDiffTree(rows: RawDiffRow[]): DiffNode | null {
  if (rows.length === 0) return null;

  const map = new Map<number, DiffNode>();
  for (const r of rows) {
    map.set(r.id, { ...r, children: [] });
  }

  let root: DiffNode | null = null;
  for (const node of map.values()) {
    if (node.parentId === -1 || !map.has(node.parentId)) {
      root = node;
    } else {
      map.get(node.parentId)!.children.push(node);
    }
  }

  function sort(n: DiffNode): void {
    n.children.sort((a, b) => b.regressionNs - a.regressionNs);
    for (const c of n.children) sort(c);
  }
  if (root) sort(root);

  return root;
}
