import * as PIXI from 'pixi.js';
import type { DrawLine, Viewport } from '../types';
import type { SceneLayer } from '../SceneLayer';
import { worldXToScreen, worldYToScreen } from '../viewport';

/** Renders a batch of world-space line segments. */
export class LineLayer implements SceneLayer {
  readonly container: PIXI.Container;
  private graphics: PIXI.Graphics;
  private lines: DrawLine[] = [];

  constructor() {
    this.container = new PIXI.Container();
    this.graphics = new PIXI.Graphics();
    this.container.addChild(this.graphics);
  }

  setLines(lines: DrawLine[]): void {
    this.lines = lines;
  }

  render(vp: Viewport): void {
    this.graphics.clear();

    for (const l of this.lines) {
      const x0 = worldXToScreen(vp, l.x0);
      const y0 = worldYToScreen(vp, l.y0);
      const x1 = worldXToScreen(vp, l.x1);
      const y1 = worldYToScreen(vp, l.y1);

      // Skip lines entirely outside the canvas
      const margin = 2;
      const inX = !(Math.max(x0, x1) < -margin || Math.min(x0, x1) > vp.width + margin);
      const inY = !(Math.max(y0, y1) < -margin || Math.min(y0, y1) > vp.height + margin);
      if (!inX && !inY) continue;

      this.graphics.lineStyle(l.width ?? 1, l.color, 1);
      this.graphics.moveTo(x0, y0);
      this.graphics.lineTo(x1, y1);
    }
  }

  destroy(): void {
    this.graphics.destroy();
    this.container.destroy();
  }
}
