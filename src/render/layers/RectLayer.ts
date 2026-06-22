import * as PIXI from 'pixi.js';
import type { DrawRect, Viewport } from '../types';
import type { SceneLayer } from '../SceneLayer';
import { isRectVisible, worldXToScreen, worldYToScreen, worldWidthToScreen, worldHeightToScreen } from '../viewport';

/**
 * Renders a batch of world-space rectangles using a single PIXI.Graphics object
 * (one GPU draw call per fill color group — PixiJS batches by fill state).
 *
 * Handles viewport culling: only rects that intersect the viewport are drawn.
 */
export class RectLayer implements SceneLayer {
  readonly container: PIXI.Container;
  private graphics: PIXI.Graphics;
  private rects: DrawRect[] = [];

  constructor() {
    this.container = new PIXI.Container();
    this.graphics = new PIXI.Graphics();
    this.container.addChild(this.graphics);
  }

  setRects(rects: DrawRect[]): void {
    this.rects = rects;
  }

  render(vp: Viewport): void {
    this.graphics.clear();

    for (const r of this.rects) {
      if (!isRectVisible(vp, r.x, r.y, r.w, r.h)) continue;

      const sx = worldXToScreen(vp, r.x);
      const sy = worldYToScreen(vp, r.y);
      const sw = worldWidthToScreen(vp, r.w);
      const sh = worldHeightToScreen(vp, r.h);

      // Skip sub-pixel rects (LOD: invisible at this zoom)
      if (sw < 0.5 || sh < 0.5) continue;

      this.graphics.beginFill(r.color, r.alpha ?? 1);
      this.graphics.drawRect(sx, sy, sw, sh);
      this.graphics.endFill();
    }
  }

  destroy(): void {
    this.graphics.destroy();
    this.container.destroy();
  }
}
