import { describe, expect, it, beforeEach } from 'vitest';
import { HitTester } from '../hit-tester';
import type { DrawRect, Viewport } from '../types';

const VP: Viewport = {
  tpp: 1,
  vpp: 1,
  left: 0,
  top: 0,
  width: 800,
  height: 600,
};

function rect(id: number, x: number, y: number, w: number, h: number, color = 0xffffff): DrawRect {
  return { id, x, y, w, h, color };
}

describe('HitTester', () => {
  let ht: HitTester;

  beforeEach(() => {
    ht = new HitTester();
  });

  it('returns null when tree is empty', () => {
    expect(ht.hitTest(VP, 100, 100)).toBeNull();
  });

  it('hits a rect', () => {
    ht.load([rect(1, 0, 0, 100, 50)]);
    const result = ht.hitTest(VP, 50, 25);
    expect(result?.id).toBe(1);
  });

  it('misses outside rect', () => {
    ht.load([rect(1, 0, 0, 100, 50)]);
    expect(ht.hitTest(VP, 200, 200)).toBeNull();
  });

  it('returns deepest rect when rects overlap (highest y)', () => {
    ht.load([
      rect(1, 0, 0, 100, 100),
      rect(2, 10, 20, 50, 50), // higher y (deeper in scene)
    ]);
    const result = ht.hitTest(VP, 30, 40);
    expect(result?.id).toBe(2);
  });

  it('clear removes all entries', () => {
    ht.load([rect(1, 0, 0, 100, 50)]);
    ht.clear();
    expect(ht.hitTest(VP, 50, 25)).toBeNull();
  });

  it('query returns rects in area', () => {
    ht.load([
      rect(1, 0, 0, 50, 50),
      rect(2, 200, 200, 50, 50),
    ]);
    const results = ht.query(0, 0, 100, 100);
    expect(results).toHaveLength(1);
    expect(results[0].id).toBe(1);
  });

  it('handles large batches without error', () => {
    const rects: DrawRect[] = Array.from({ length: 10_000 }, (_, i) =>
      rect(i, i * 2, 0, 1.5, 20),
    );
    ht.load(rects);
    expect(ht.hitTest(VP, 5_000, 10)?.id).toBeDefined();
  });
});
