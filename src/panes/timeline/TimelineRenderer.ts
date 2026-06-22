import type { DrawLine, DrawRect, DrawText, SceneFrame, Viewport } from '../../render/types';
import { worldWidthToScreen } from '../../render/viewport';
import { formatNs, makeTimeScale } from '../../render/scales';
import type { Track, TimelineEvent } from './types';
import { kindColor, RULER_HEIGHT, TRACK_HEIGHT } from './types';

/** Pixel width below which individual events are merged into a density strip */
const LOD_MERGE_THRESHOLD = 1.5;

/** Max events per density bucket (pixel column) before saturating color */
const DENSITY_SATURATE = 10;

/**
 * Builds the SceneFrame for one timeline render pass.
 *
 * Separating layout from PixiJS allows unit testing without a DOM.
 */
export function buildTimelineFrame(tracks: Track[], vp: Viewport, selRange: [number, number] | null): SceneFrame {
  const rects: DrawRect[] = [];
  const lines: DrawLine[] = [];
  const texts: DrawText[] = [];

  buildRuler(vp, rects, lines, texts);

  let id = 1;
  for (const track of tracks) {
    id = buildTrack(track, vp, rects, texts, id);
  }

  if (selRange) {
    buildSelectionOverlay(selRange, vp, rects);
  }

  return { rects, lines, texts };
}

function buildRuler(vp: Viewport, rects: DrawRect[], lines: DrawLine[], texts: DrawText[]): void {
  // Background bar
  rects.push({ id: 0, x: vp.left, y: vp.top, w: vp.width * vp.tpp, h: RULER_HEIGHT * vp.vpp, color: 0x0d1117, alpha: 1 });

  const scale = makeTimeScale(vp);
  const ticks = scale.ticks(Math.floor(vp.width / 80));

  for (const t of ticks) {
    const sx = scale(t);
    const wx = vp.left + sx * vp.tpp;
    // Tick line
    lines.push({ x0: wx, y0: vp.top, x1: wx, y1: vp.top + RULER_HEIGHT * vp.vpp, color: 0x30363d });
    // Label
    texts.push({ x: wx, y: vp.top + 6 * vp.vpp, text: formatNs(t), color: 0x8b949e, fontSize: 10 });
  }
}

function buildTrack(track: Track, vp: Viewport, rects: DrawRect[], texts: DrawText[], nextId: number): number {
  let id = nextId;
  const trackTopWorld = track.yOffset;

  // Track background
  rects.push({
    id: 0,
    x: vp.left,
    y: trackTopWorld,
    w: vp.width * vp.tpp,
    h: track.height * vp.vpp,
    color: 0x161b22,
    alpha: 1,
  });

  // Track label (left-fixed, drawn at world left edge)
  texts.push({
    x: vp.left + 4 * vp.tpp,
    y: trackTopWorld + 4 * vp.vpp,
    text: track.label,
    color: 0x8b949e,
    fontSize: 10,
  });

  // Decide LOD mode: if the average event is narrower than threshold, use density
  const vpSpanNs = vp.width * vp.tpp;
  const eventPx = vpSpanNs > 0 ? (track.events.length * TRACK_HEIGHT) / (vp.width) : Infinity;

  if (eventPx > LOD_MERGE_THRESHOLD * 10) {
    id = buildDensityStrip(track, vp, rects, id);
  } else {
    id = buildIndividualEvents(track, vp, rects, texts, id);
  }

  return id;
}

function buildIndividualEvents(
  track: Track,
  vp: Viewport,
  rects: DrawRect[],
  texts: DrawText[],
  nextId: number,
): number {
  let id = nextId;
  for (const ev of track.events) {
    const right = vp.left + vp.width * vp.tpp;
    if (ev.timeNs + ev.durationNs < vp.left || ev.timeNs > right) continue;

    const y = track.yOffset + ev.depth * (TRACK_HEIGHT * vp.vpp);
    const sw = worldWidthToScreen(vp, ev.durationNs);

    rects.push({
      id,
      x: ev.timeNs,
      y,
      w: ev.durationNs,
      h: TRACK_HEIGHT * vp.vpp,
      color: kindColor(ev.kind),
      alpha: 0.9,
    });

    if (sw >= 40) {
      texts.push({
        x: ev.timeNs,
        y,
        text: ev.name,
        color: 0xffffff,
        fontSize: 10,
        minPx: sw,
      });
    }

    id++;
  }
  return id;
}

function buildDensityStrip(track: Track, vp: Viewport, rects: DrawRect[], nextId: number): number {
  const buckets = new Float32Array(Math.ceil(vp.width));
  const rightNs = vp.left + vp.width * vp.tpp;

  for (const ev of track.events) {
    if (ev.timeNs + ev.durationNs < vp.left || ev.timeNs > rightNs) continue;
    const col = Math.floor((ev.timeNs - vp.left) / vp.tpp);
    if (col >= 0 && col < buckets.length) {
      buckets[col] = Math.min(buckets[col] + 1, DENSITY_SATURATE);
    }
  }

  let id = nextId;
  for (let col = 0; col < buckets.length; col++) {
    if (buckets[col] === 0) continue;
    const alpha = buckets[col] / DENSITY_SATURATE;
    rects.push({
      id,
      x: vp.left + col * vp.tpp,
      y: track.yOffset,
      w: vp.tpp,
      h: track.height * vp.vpp,
      color: 0x4e9af1,
      alpha: 0.3 + alpha * 0.7,
    });
    id++;
  }
  return id;
}

function buildSelectionOverlay(range: [number, number], vp: Viewport, rects: DrawRect[]): void {
  rects.push({
    id: 0,
    x: range[0],
    y: vp.top,
    w: range[1] - range[0],
    h: vp.height * vp.vpp,
    color: 0x4e9af1,
    alpha: 0.15,
  });
}
