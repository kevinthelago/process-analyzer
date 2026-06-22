import type { CallNode, FlamegraphRect } from './types';
import { FRAME_HEIGHT, frameColor } from './types';

export interface LayoutOptions {
  /** Canvas width in pixels */
  width: number;
  /** Root total time (used to compute proportional widths) */
  rootTotalNs: number;
  /** X offset of the current zoom focus (from zoom-to-frame) */
  focusX: number;
  /** Width of the current zoom focus in pixels (1 = full width) */
  focusW: number;
  /** Search query — matching frames get highlighted */
  searchQuery: string;
}

/**
 * Lay out a flame graph tree top-down (root at top, children below).
 *
 * Width of each node = (node.totalNs / root.totalNs) * canvasWidth, scaled
 * by the current zoom focus. Skips nodes narrower than 1px.
 */
export function layoutFlamegraph(root: CallNode, opts: LayoutOptions): FlamegraphRect[] {
  const rects: FlamegraphRect[] = [];
  const scale = opts.width / opts.rootTotalNs;
  const query = opts.searchQuery.toLowerCase().trim();

  function visit(node: CallNode, x: number, depth: number): void {
    const w = node.totalNs * scale;
    if (w < 1) return;

    const y = depth * FRAME_HEIGHT;
    const highlighted = query.length > 0 && node.frame.toLowerCase().includes(query);

    rects.push({
      nodeId: node.id,
      frame: node.frame,
      selfNs: node.selfNs,
      totalNs: node.totalNs,
      x,
      y,
      w,
      h: FRAME_HEIGHT - 1,
      color: frameColor(node.frame),
      highlighted,
    });

    let childX = x;
    for (const child of node.children) {
      visit(child, childX, depth + 1);
      childX += child.totalNs * scale;
    }
  }

  visit(root, 0, 0);
  return rects;
}

/** Build a call tree from a flat Arrow table output */
export function buildCallTree(rows: RawRow[]): CallNode | null {
  if (rows.length === 0) return null;

  const nodeMap = new Map<number, CallNode>();
  for (const r of rows) {
    nodeMap.set(r.id, {
      id: r.id,
      parentId: r.parentId,
      frame: r.frame,
      selfNs: r.selfNs,
      totalNs: r.totalNs,
      depth: r.depth,
      children: [],
    });
  }

  let root: CallNode | null = null;
  for (const node of nodeMap.values()) {
    if (node.parentId === -1 || !nodeMap.has(node.parentId)) {
      root = node;
    } else {
      nodeMap.get(node.parentId)!.children.push(node);
    }
  }

  // Sort children by totalNs descending so widest frames appear first
  function sortChildren(n: CallNode): void {
    n.children.sort((a, b) => b.totalNs - a.totalNs);
    for (const c of n.children) sortChildren(c);
  }
  if (root) sortChildren(root);

  return root;
}

export interface RawRow {
  id: number;
  parentId: number;
  frame: string;
  selfNs: number;
  totalNs: number;
  depth: number;
}
