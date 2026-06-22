import type { DrawRect, DrawLine, DrawText, HitResult, RendererOptions, SceneFrame, Viewport } from './types';
import { HitTester } from './hit-tester';
import { isRectVisible, worldXToScreen, worldYToScreen, worldWidthToScreen, worldHeightToScreen } from './viewport';

/**
 * Canvas2D fallback renderer with the same public interface as PixiRenderer.
 * Used when WebGL is unavailable (old hardware / headless test environments).
 */
export class Canvas2dFallback {
  private ctx: CanvasRenderingContext2D;
  private hitTester: HitTester;
  private bg: string;

  constructor(canvas: HTMLCanvasElement, opts: RendererOptions = {}) {
    const ctx = canvas.getContext('2d');
    if (!ctx) throw new Error('Canvas2D not supported');
    this.ctx = ctx;
    this.hitTester = new HitTester();
    this.bg = '#' + ((opts.backgroundColor ?? 0x1a1a2e) >>> 0).toString(16).padStart(6, '0');
  }

  render(frame: SceneFrame, vp: Viewport): void {
    const { ctx } = this;
    ctx.fillStyle = this.bg;
    ctx.fillRect(0, 0, vp.width, vp.height);

    this.drawLines(frame.lines, vp);
    this.drawRects(frame.rects, vp);
    this.drawTexts(frame.texts, vp);

    this.hitTester.load(frame.rects);
  }

  private drawRects(rects: DrawRect[], vp: Viewport): void {
    const { ctx } = this;
    for (const r of rects) {
      if (!isRectVisible(vp, r.x, r.y, r.w, r.h)) continue;
      const sx = worldXToScreen(vp, r.x);
      const sy = worldYToScreen(vp, r.y);
      const sw = worldWidthToScreen(vp, r.w);
      const sh = worldHeightToScreen(vp, r.h);
      if (sw < 0.5 || sh < 0.5) continue;
      ctx.globalAlpha = r.alpha ?? 1;
      ctx.fillStyle = '#' + r.color.toString(16).padStart(6, '0');
      ctx.fillRect(sx, sy, sw, sh);
    }
    ctx.globalAlpha = 1;
  }

  private drawLines(lines: DrawLine[], vp: Viewport): void {
    const { ctx } = this;
    for (const l of lines) {
      const x0 = worldXToScreen(vp, l.x0);
      const y0 = worldYToScreen(vp, l.y0);
      const x1 = worldXToScreen(vp, l.x1);
      const y1 = worldYToScreen(vp, l.y1);
      ctx.strokeStyle = '#' + l.color.toString(16).padStart(6, '0');
      ctx.lineWidth = l.width ?? 1;
      ctx.beginPath();
      ctx.moveTo(x0, y0);
      ctx.lineTo(x1, y1);
      ctx.stroke();
    }
  }

  private drawTexts(texts: DrawText[], vp: Viewport): void {
    const { ctx } = this;
    ctx.font = '11px ui-monospace, monospace';
    for (const t of texts) {
      if (t.minPx !== undefined && t.minPx < 6) continue;
      const sx = worldXToScreen(vp, t.x);
      const sy = worldYToScreen(vp, t.y);
      if (sx > vp.width || sy > vp.height || sx < -200) continue;
      ctx.fillStyle = '#' + (t.color ?? 0xeeeeee).toString(16).padStart(6, '0');
      ctx.fillText(t.text, sx + 2, sy + 13);
    }
  }

  hitTest(vp: Viewport, screenX: number, screenY: number): HitResult | null {
    return this.hitTester.hitTest(vp, screenX, screenY);
  }

  resize(_width: number, _height: number): void {
    // Canvas2D resizes automatically via CSS
  }

  readonly rendererType = 'canvas2d';

  destroy(): void {
    this.hitTester.clear();
  }
}
