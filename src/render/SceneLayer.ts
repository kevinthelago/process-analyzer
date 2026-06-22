import type * as PIXI from 'pixi.js';
import type { Viewport } from './types';

/**
 * Base interface for all render layers. Each layer owns a PIXI.Container and
 * redraws it from scratch on each render() call. Layers are composited by the
 * PixiRenderer in insertion order.
 */
export interface SceneLayer {
  readonly container: PIXI.Container;
  /** Redraw layer contents for the new viewport */
  render(vp: Viewport): void;
  /** Release GPU resources */
  destroy(): void;
}
