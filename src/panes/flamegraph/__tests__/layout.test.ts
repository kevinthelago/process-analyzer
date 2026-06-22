import { describe, expect, it } from 'vitest';
import { buildCallTree, layoutFlamegraph } from '../layout';
import type { RawRow } from '../layout';

function makeRow(id: number, parentId: number, frame: string, selfNs: number, totalNs: number, depth: number): RawRow {
  return { id, parentId, frame, selfNs, totalNs, depth };
}

describe('buildCallTree', () => {
  it('returns null for empty input', () => {
    expect(buildCallTree([])).toBeNull();
  });

  it('builds a single-node tree', () => {
    const root = buildCallTree([makeRow(0, -1, 'main', 100, 100, 0)]);
    expect(root).not.toBeNull();
    expect(root!.frame).toBe('main');
    expect(root!.children).toHaveLength(0);
  });

  it('links children to parent', () => {
    const rows = [
      makeRow(0, -1, 'main', 10, 100, 0),
      makeRow(1, 0, 'foo', 40, 60, 1),
      makeRow(2, 0, 'bar', 30, 30, 1),
    ];
    const root = buildCallTree(rows)!;
    expect(root.children).toHaveLength(2);
  });

  it('sorts children by totalNs descending', () => {
    const rows = [
      makeRow(0, -1, 'main', 0, 100, 0),
      makeRow(1, 0, 'small', 5, 20, 1),
      makeRow(2, 0, 'big', 5, 70, 1),
    ];
    const root = buildCallTree(rows)!;
    expect(root.children[0].frame).toBe('big');
    expect(root.children[1].frame).toBe('small');
  });
});

describe('layoutFlamegraph', () => {
  const root = buildCallTree([
    makeRow(0, -1, 'root', 0, 1000, 0),
    makeRow(1, 0, 'alpha', 200, 700, 1),
    makeRow(2, 0, 'beta', 300, 300, 1),
  ])!;

  const opts = { width: 1000, rootTotalNs: 1000, focusX: 0, focusW: 1, searchQuery: '' };

  it('root rect spans full width', () => {
    const rects = layoutFlamegraph(root, opts);
    const rootRect = rects.find((r) => r.frame === 'root')!;
    expect(rootRect.w).toBeCloseTo(1000);
    expect(rootRect.x).toBeCloseTo(0);
  });

  it('child widths sum to root width', () => {
    const rects = layoutFlamegraph(root, opts);
    const children = rects.filter((r) => r.frame === 'alpha' || r.frame === 'beta');
    const totalW = children.reduce((s, r) => s + r.w, 0);
    expect(totalW).toBeCloseTo(1000);
  });

  it('frames at deeper depth have greater y', () => {
    const rects = layoutFlamegraph(root, opts);
    const rootR = rects.find((r) => r.frame === 'root')!;
    const childR = rects.find((r) => r.frame === 'alpha')!;
    expect(childR.y).toBeGreaterThan(rootR.y);
  });

  it('highlights matching frames', () => {
    const rects = layoutFlamegraph(root, { ...opts, searchQuery: 'alpha' });
    const alpha = rects.find((r) => r.frame === 'alpha')!;
    const beta = rects.find((r) => r.frame === 'beta')!;
    expect(alpha.highlighted).toBe(true);
    expect(beta.highlighted).toBe(false);
  });

  it('skips frames narrower than 1px', () => {
    const tiny = buildCallTree([
      makeRow(0, -1, 'root', 0, 1000, 0),
      makeRow(1, 0, 'micro', 0, 1, 1), // 1ns / 1000 * 100px = 0.1px
    ])!;
    const rects = layoutFlamegraph(tiny, { ...opts, width: 100 });
    expect(rects.some((r) => r.frame === 'micro')).toBe(false);
  });
});
