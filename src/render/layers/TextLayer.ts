import * as PIXI from 'pixi.js';
import type { DrawText, Viewport } from '../types';
import type { SceneLayer } from '../SceneLayer';
import { worldXToScreen, worldYToScreen } from '../viewport';

const STYLE_CACHE = new Map<string, PIXI.TextStyle>();

function getStyle(color: number, fontSize: number): PIXI.TextStyle {
  const key = `${color}:${fontSize}`;
  if (!STYLE_CACHE.has(key)) {
    STYLE_CACHE.set(
      key,
      new PIXI.TextStyle({
        fontFamily: 'ui-monospace, monospace',
        fontSize,
        fill: color,
        resolution: 2,
      }),
    );
  }
  return STYLE_CACHE.get(key)!;
}

/**
 * Renders text labels with LOD. Each call to render() recycles existing
 * PIXI.Text objects to avoid GC pressure.
 *
 * Text with minPx set is hidden when the world-space parent element is
 * narrower than that many screen pixels.
 */
export class TextLayer implements SceneLayer {
  readonly container: PIXI.Container;
  private pool: PIXI.Text[] = [];
  private active = 0;
  private texts: DrawText[] = [];

  constructor() {
    this.container = new PIXI.Container();
  }

  setTexts(texts: DrawText[]): void {
    this.texts = texts;
  }

  render(vp: Viewport): void {
    this.active = 0;

    for (const t of this.texts) {
      const sx = worldXToScreen(vp, t.x);
      const sy = worldYToScreen(vp, t.y);

      if (sx > vp.width || sy > vp.height || sx < -200 || sy < -20) continue;
      if (t.minPx !== undefined && t.minPx < 6) continue;

      const fontSize = t.fontSize ?? 11;
      const color = t.color ?? 0xeeeeee;
      const sprite = this.acquire();
      sprite.text = t.text;
      sprite.style = getStyle(color, fontSize);
      sprite.x = sx + 2;
      sprite.y = sy + 2;
      sprite.visible = true;
    }

    // Hide unused pool entries
    for (let i = this.active; i < this.pool.length; i++) {
      this.pool[i].visible = false;
    }
  }

  private acquire(): PIXI.Text {
    if (this.active < this.pool.length) {
      return this.pool[this.active++];
    }
    const t = new PIXI.Text('', getStyle(0xeeeeee, 11));
    this.pool.push(t);
    this.container.addChild(t);
    this.active++;
    return t;
  }

  destroy(): void {
    for (const t of this.pool) t.destroy();
    this.container.destroy();
    STYLE_CACHE.clear();
  }
}
