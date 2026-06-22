import * as PIXI from 'pixi.js';
import { HitTester } from './hit-tester';
import { RectLayer } from './layers/RectLayer';
import { LineLayer } from './layers/LineLayer';
import { TextLayer } from './layers/TextLayer';
import type { HitResult, RendererOptions, SceneFrame, Viewport } from './types';

/**
 * Main WebGL renderer backed by PixiJS.
 *
 * Usage:
 *   const r = new PixiRenderer(canvasEl);
 *   r.render(frame, viewport);          // called each animation frame
 *   r.hitTest(viewport, sx, sy);        // called on pointer events
 *   r.destroy();                        // called on unmount
 */
export class PixiRenderer {
  private app: PIXI.Application;
  private rectLayer: RectLayer;
  private lineLayer: LineLayer;
  private textLayer: TextLayer;
  private hitTester: HitTester;

  constructor(canvas: HTMLCanvasElement, opts: RendererOptions = {}) {
    this.app = new PIXI.Application({
      view: canvas,
      width: canvas.clientWidth,
      height: canvas.clientHeight,
      backgroundColor: opts.backgroundColor ?? 0x1a1a2e,
      antialias: false,
      resolution: opts.resolution ?? (window.devicePixelRatio || 1),
      autoDensity: true,
      forceCanvas: opts.forceCanvas ?? false,
    });

    this.rectLayer = new RectLayer();
    this.lineLayer = new LineLayer();
    this.textLayer = new TextLayer();
    this.hitTester = new HitTester();

    this.app.stage.addChild(this.rectLayer.container);
    this.app.stage.addChild(this.lineLayer.container);
    this.app.stage.addChild(this.textLayer.container);

    // Stop PixiJS's internal ticker; we drive rendering manually via render()
    this.app.ticker.stop();
  }

  /** Synchronously render a scene frame. Call from requestAnimationFrame. */
  render(frame: SceneFrame, vp: Viewport): void {
    this.rectLayer.setRects(frame.rects);
    this.lineLayer.setLines(frame.lines);
    this.textLayer.setTexts(frame.texts);

    this.rectLayer.render(vp);
    this.lineLayer.render(vp);
    this.textLayer.render(vp);

    this.hitTester.load(frame.rects);

    this.app.renderer.render(this.app.stage);
  }

  hitTest(vp: Viewport, screenX: number, screenY: number): HitResult | null {
    return this.hitTester.hitTest(vp, screenX, screenY);
  }

  resize(width: number, height: number): void {
    this.app.renderer.resize(width, height);
  }

  get rendererType(): string {
    return this.app.renderer.type === PIXI.RENDERER_TYPE.WEBGL ? 'webgl' : 'canvas2d';
  }

  destroy(): void {
    this.rectLayer.destroy();
    this.lineLayer.destroy();
    this.textLayer.destroy();
    this.hitTester.clear();
    this.app.destroy(false, { children: true });
  }
}
