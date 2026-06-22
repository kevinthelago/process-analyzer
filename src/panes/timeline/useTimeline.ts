import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { tableFromIPC } from 'apache-arrow';
import type { Viewport } from '../../render/types';
import type { TimeRange } from '../../contracts/selection-store';
import { fitX, pan, zoomX } from '../../render/viewport';
import type { Track, TimelineEvent } from './types';
import { RULER_HEIGHT, TRACK_GAP, TRACK_HEIGHT } from './types';

interface UseTimelineOptions {
  pid: number | null;
  tid: number | null;
  width: number;
  height: number;
}

interface TimelineState {
  tracks: Track[];
  viewport: Viewport;
  isLoading: boolean;
  totalSpanNs: TimeRange;
}

interface TimelineActions {
  panBy: (dx: number) => void;
  zoomAt: (screenX: number, factor: number) => void;
  resetZoom: () => void;
}

export function useTimeline(opts: UseTimelineOptions): [TimelineState, TimelineActions] {
  const [tracks, setTracks] = useState<Track[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [spanNs, setSpanNs] = useState<TimeRange>({ startNs: 0, endNs: 1_000_000 });
  const vpRef = useRef<Viewport>(fitX(opts.width, opts.height, 0, 1_000_000));
  const [viewport, setViewport] = useState<Viewport>(vpRef.current);

  // Fetch events from Rust via Tauri IPC (Arrow IPC buffer)
  useEffect(() => {
    let cancelled = false;
    setIsLoading(true);

    invoke<ArrayBuffer>('query_timeline_events', {
      pid: opts.pid ?? null,
      tid: opts.tid ?? null,
    })
      .then((buf) => {
        if (cancelled) return;
        const table = tableFromIPC(new Uint8Array(buf));
        const events = arrowTableToEvents(table);
        const built = buildTracks(events, opts.height);
        setTracks(built.tracks);
        setSpanNs(built.spanNs);

        const vp = fitX(opts.width, opts.height, built.spanNs.startNs, built.spanNs.endNs);
        vpRef.current = vp;
        setViewport(vp);
      })
      .catch(console.error)
      .finally(() => {
        if (!cancelled) setIsLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, [opts.pid, opts.tid, opts.width, opts.height]);

  const panBy = useCallback((dx: number) => {
    const next = pan(vpRef.current, { dx, dy: 0 });
    vpRef.current = next;
    setViewport(next);
  }, []);

  const zoomAt = useCallback((screenX: number, factor: number) => {
    const next = zoomX(vpRef.current, screenX, factor);
    vpRef.current = next;
    setViewport(next);
  }, []);

  const resetZoom = useCallback(() => {
    const next = fitX(opts.width, opts.height, spanNs.startNs, spanNs.endNs);
    vpRef.current = next;
    setViewport(next);
  }, [opts.width, opts.height, spanNs]);

  return [
    { tracks, viewport, isLoading, totalSpanNs: spanNs },
    { panBy, zoomAt, resetZoom },
  ];
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

function arrowTableToEvents(table: ReturnType<typeof tableFromIPC>): TimelineEvent[] {
  const events: TimelineEvent[] = [];
  for (let i = 0; i < table.numRows; i++) {
    events.push({
      timeNs: Number(table.getChildAt(0)?.get(i) ?? 0),
      durationNs: Number(table.getChildAt(1)?.get(i) ?? 0),
      pid: Number(table.getChildAt(2)?.get(i) ?? 0),
      tid: Number(table.getChildAt(3)?.get(i) ?? 0),
      name: String(table.getChildAt(4)?.get(i) ?? ''),
      kind: String(table.getChildAt(5)?.get(i) ?? 'cpu'),
      depth: Number(table.getChildAt(6)?.get(i) ?? 0),
    });
  }
  return events;
}

function buildTracks(
  events: TimelineEvent[],
  _canvasHeight: number,
): { tracks: Track[]; spanNs: TimeRange } {
  if (events.length === 0) return { tracks: [], spanNs: { startNs: 0, endNs: 1_000_000 } };

  // Group by tid
  const byTid = new Map<number, TimelineEvent[]>();
  for (const ev of events) {
    const key = ev.tid;
    if (!byTid.has(key)) byTid.set(key, []);
    byTid.get(key)!.push(ev);
  }

  let minNs = Infinity;
  let maxNs = -Infinity;
  for (const ev of events) {
    if (ev.timeNs < minNs) minNs = ev.timeNs;
    if (ev.timeNs + ev.durationNs > maxNs) maxNs = ev.timeNs + ev.durationNs;
  }

  let y = RULER_HEIGHT;
  const tracks: Track[] = [];

  for (const [tid, evs] of byTid) {
    const maxDepth = evs.reduce((m, e) => Math.max(m, e.depth), 0);
    const trackH = (maxDepth + 1) * TRACK_HEIGHT + TRACK_GAP;
    const pid = evs[0].pid;

    tracks.push({
      label: `pid ${pid} · tid ${tid}`,
      pid,
      tid,
      events: evs,
      yOffset: y,
      height: trackH,
    });

    y += trackH;
  }

  return { tracks, spanNs: { startNs: minNs, endNs: maxNs } };
}
