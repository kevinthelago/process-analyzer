import RBush from 'rbush';
import type { DrawRect, HitResult, Viewport } from './types';
import { screenXToWorld, screenYToWorld } from './viewport';

interface Entry {
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
  rect: DrawRect;
}

/**
 * Spatial index over DrawRect world-space coordinates.
 *
 * Rebuilt on each render frame so it always reflects the current scene.
 * RBush bulk-load is O(n log n) and in practice fast enough for ≤ 100k rects.
 */
export class HitTester {
  private tree = new RBush<Entry>();

  load(rects: DrawRect[]): void {
    const entries: Entry[] = rects.map((r) => ({
      minX: r.x,
      minY: r.y,
      maxX: r.x + r.w,
      maxY: r.y + r.h,
      rect: r,
    }));
    this.tree.clear();
    if (entries.length > 0) {
      this.tree.load(entries);
    }
  }

  /**
   * Hit-test a screen-space point. Returns the topmost (highest y within the
   * deepest matching layer, i.e. last in array order) intersecting rect.
   */
  hitTest(vp: Viewport, screenX: number, screenY: number): HitResult | null {
    const wx = screenXToWorld(vp, screenX);
    const wy = screenYToWorld(vp, screenY);

    const candidates = this.tree.search({
      minX: wx,
      minY: wy,
      maxX: wx,
      maxY: wy,
    });

    if (candidates.length === 0) return null;

    // Return the candidate with the highest minY (deepest in the scene)
    const best = candidates.reduce((a, b) => (a.minY >= b.minY ? a : b));
    return { id: best.rect.id, rect: best.rect };
  }

  /**
   * Return all rects whose world-space bounds intersect the given world-space
   * query rect.
   */
  query(wx: number, wy: number, ww: number, wh: number): DrawRect[] {
    return this.tree
      .search({ minX: wx, minY: wy, maxX: wx + ww, maxY: wy + wh })
      .map((e) => e.rect);
  }

  clear(): void {
    this.tree.clear();
  }
}
