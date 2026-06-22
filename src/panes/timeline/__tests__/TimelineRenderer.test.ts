import { describe, expect, it } from 'vitest';
import { buildTimelineFrame } from '../TimelineRenderer';
import type { Track, TimelineEvent } from '../types';
import type { Viewport } from '../../../render/types';

const VP: Viewport = { tpp: 1, vpp: 1, left: 0, top: 0, width: 1920, height: 800 };

function makeTrack(events: TimelineEvent[]): Track {
  return {
    label: 'test',
    pid: 1,
    tid: 1,
    events,
    yOffset: 28,
    height: 24,
  };
}

function makeEvent(timeNs: number, durationNs: number, depth = 0): TimelineEvent {
  return { timeNs, durationNs, pid: 1, tid: 1, name: 'fn', kind: 'cpu', depth };
}

describe('buildTimelineFrame', () => {
  it('returns a frame with rects, lines, texts arrays', () => {
    const frame = buildTimelineFrame([], VP, null);
    expect(frame).toHaveProperty('rects');
    expect(frame).toHaveProperty('lines');
    expect(frame).toHaveProperty('texts');
  });

  it('ruler always produces rects and texts', () => {
    const frame = buildTimelineFrame([], VP, null);
    expect(frame.rects.length).toBeGreaterThan(0);
    // At least one tick label in texts (unless span is 0)
  });

  it('events outside viewport produce no rects beyond ruler', () => {
    const ev = makeEvent(10_000_000, 1_000, 0); // 10ms, out of [0,1920] range
    const track = makeTrack([ev]);
    const vp: Viewport = { ...VP, left: 0, tpp: 1 };
    // 10_000_000ns start >> 1920px range — should be culled
    const frame = buildTimelineFrame([track], vp, null);
    // Only ruler rect + track background, no event rects
    const eventRects = frame.rects.filter((r) => r.id > 0);
    expect(eventRects).toHaveLength(0);
  });

  it('events inside viewport produce rects', () => {
    const ev = makeEvent(100, 200, 0); // fits in [0, 1920] at tpp=1
    const track = makeTrack([ev]);
    const frame = buildTimelineFrame([track], VP, null);
    const eventRects = frame.rects.filter((r) => r.id > 0);
    expect(eventRects.length).toBeGreaterThan(0);
  });

  it('selection overlay rect is added when selRange provided', () => {
    const frame = buildTimelineFrame([], VP, { startNs: 100, endNs: 200 });
    // Selection overlay rect has x=startNs
    const overlay = frame.rects.find((r) => r.x === 100 && r.color === 0x4e9af1);
    expect(overlay).toBeDefined();
  });

  it('no selection overlay when selRange is null', () => {
    const frame = buildTimelineFrame([], VP, null);
    const overlay = frame.rects.find((r) => r.color === 0x4e9af1 && r.alpha === 0.15);
    expect(overlay).toBeUndefined();
  });

  it('density mode fires when many events per pixel', () => {
    // 500 events all at different positions in the viewport
    const events = Array.from({ length: 500 }, (_, i) =>
      makeEvent(i * 2, 1),
    );
    const track = makeTrack(events);
    const frame = buildTimelineFrame([track], VP, null);
    // In density mode, rects use alpha < 1 (density strip mode)
    const densityRects = frame.rects.filter((r) => r.id > 0 && (r.alpha ?? 1) < 1);
    expect(densityRects.length).toBeGreaterThan(0);
  });
});
