/** RGBA color as 0xRRGGBB (alpha separate) */
export type Color = number;

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface DrawRect {
  /** World-space x position */
  x: number;
  /** World-space y position (pixels from top of scene) */
  y: number;
  /** World-space width */
  w: number;
  /** World-space height */
  h: number;
  color: Color;
  alpha?: number;
  /** Opaque numeric id returned from hit-test */
  id: number;
}

export interface DrawLine {
  x0: number;
  y0: number;
  x1: number;
  y1: number;
  color: Color;
  width?: number;
}

export interface DrawText {
  x: number;
  y: number;
  text: string;
  color?: Color;
  fontSize?: number;
  /**
   * Minimum screen-space width (pixels) of the parent element before this
   * text is rendered. Used for LOD — text is hidden when too small to read.
   */
  minPx?: number;
}

/** A complete set of primitives for one frame */
export interface SceneFrame {
  rects: DrawRect[];
  lines: DrawLine[];
  texts: DrawText[];
}

/** Immutable viewport snapshot */
export interface Viewport {
  /** Time units per pixel (horizontal zoom). Smaller = more zoomed in. */
  tpp: number;
  /** Value units per pixel (vertical zoom). Smaller = more zoomed in. */
  vpp: number;
  /** Left edge in world units */
  left: number;
  /** Top edge in world units */
  top: number;
  /** Canvas width in CSS pixels */
  width: number;
  /** Canvas height in CSS pixels */
  height: number;
}

export interface HitResult {
  id: number;
  rect: DrawRect;
}

export interface RendererOptions {
  /** Background color. Default 0x1a1a2e */
  backgroundColor?: Color;
  /** Device pixel ratio. Default window.devicePixelRatio */
  resolution?: number;
  /** Force canvas2d even when WebGL is available */
  forceCanvas?: boolean;
}
