import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { tableFromIPC } from 'apache-arrow';
import { buildCallTree, layoutFlamegraph, type RawRow } from './layout';
import type { CallNode, FlamegraphRect } from './types';

interface UseFlamegraphOptions {
  pid: number | null;
  tid: number | null;
  timeRange: [number, number] | null;
  width: number;
}

interface FlamegraphState {
  rects: FlamegraphRect[];
  root: CallNode | null;
  zoomedFrameId: number | null;
  searchQuery: string;
  isLoading: boolean;
}

interface FlamegraphActions {
  zoomToFrame: (nodeId: number) => void;
  resetZoom: () => void;
  setSearch: (q: string) => void;
  handleClick: (nodeId: number) => void;
}

export function useFlamegraph(opts: UseFlamegraphOptions): [FlamegraphState, FlamegraphActions] {
  const [root, setRoot] = useState<CallNode | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [zoomedFrameId, setZoomedFrameId] = useState<number | null>(null);
  const [searchQuery, setSearchQuery] = useState('');
  const [rects, setRects] = useState<FlamegraphRect[]>([]);

  // Fetch and parse call tree
  useEffect(() => {
    let cancelled = false;
    setIsLoading(true);

    invoke<ArrayBuffer>('query_call_tree', {
      pid: opts.pid ?? null,
      tid: opts.tid ?? null,
      timeRangeNs: opts.timeRange ?? null,
    })
      .then((buf) => {
        if (cancelled) return;
        const table = tableFromIPC(new Uint8Array(buf));
        const rows = arrowToRows(table);
        const tree = buildCallTree(rows);
        setRoot(tree);
        setZoomedFrameId(null);
      })
      .catch(console.error)
      .finally(() => {
        if (!cancelled) setIsLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, [opts.pid, opts.tid, opts.timeRange]);

  // Re-layout whenever root, zoom, search, or width changes
  useEffect(() => {
    if (!root) {
      setRects([]);
      return;
    }

    const focusNode = zoomedFrameId !== null ? findNode(root, zoomedFrameId) : null;
    const layoutRoot = focusNode ?? root;
    const rootTotalNs = focusNode ? focusNode.totalNs : root.totalNs;

    const laid = layoutFlamegraph(layoutRoot, {
      width: opts.width,
      rootTotalNs,
      focusX: 0,
      focusW: 1,
      searchQuery,
    });

    setRects(laid);
  }, [root, zoomedFrameId, searchQuery, opts.width]);

  const zoomToFrame = useCallback((nodeId: number) => {
    setZoomedFrameId(nodeId);
  }, []);

  const resetZoom = useCallback(() => {
    setZoomedFrameId(null);
  }, []);

  const setSearch = useCallback((q: string) => {
    setSearchQuery(q);
  }, []);

  const handleClick = useCallback(
    (nodeId: number) => {
      if (nodeId === zoomedFrameId) {
        setZoomedFrameId(null);
      } else {
        setZoomedFrameId(nodeId);
      }
    },
    [zoomedFrameId],
  );

  return [
    { rects, root, zoomedFrameId, searchQuery, isLoading },
    { zoomToFrame, resetZoom, setSearch, handleClick },
  ];
}

function arrowToRows(table: ReturnType<typeof tableFromIPC>): RawRow[] {
  const rows: RawRow[] = [];
  for (let i = 0; i < table.numRows; i++) {
    rows.push({
      id: Number(table.getChildAt(0)?.get(i) ?? i),
      parentId: Number(table.getChildAt(1)?.get(i) ?? -1),
      frame: String(table.getChildAt(2)?.get(i) ?? ''),
      selfNs: Number(table.getChildAt(3)?.get(i) ?? 0),
      totalNs: Number(table.getChildAt(4)?.get(i) ?? 0),
      depth: Number(table.getChildAt(5)?.get(i) ?? 0),
    });
  }
  return rows;
}

function findNode(node: CallNode, id: number): CallNode | null {
  if (node.id === id) return node;
  for (const child of node.children) {
    const found = findNode(child, id);
    if (found) return found;
  }
  return null;
}
