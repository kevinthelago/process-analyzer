import type { Viewport } from './types';

export interface PanDelta {
  dx: number;
  dy: number;
}

export interface ZoomAnchor {
  /** Screen-space x pivot for horizontal zoom */
  screenX: number;
  /** Screen-space y pivot for vertical zoom */
  screenY: number;
  /** Zoom factor (> 1 = zoom in, < 1 = zoom out) */
  factor: number;
}

/** Convert screen pixel x to world-space units */
export function screenXToWorld(vp: Viewport, sx: number): number {
  return vp.left + sx * vp.tpp;
}

/** Convert screen pixel y to world-space units */
export function screenYToWorld(vp: Viewport, sy: number): number {
  return vp.top + sy * vp.vpp;
}

/** Convert world-space unit to screen pixel x */
export function worldXToScreen(vp: Viewport, wx: number): number {
  return (wx - vp.left) / vp.tpp;
}

/** Convert world-space unit to screen pixel y */
export function worldYToScreen(vp: Viewport, wy: number): number {
  return (wy - vp.top) / vp.vpp;
}

/** Convert world-space width to screen pixels */
export function worldWidthToScreen(vp: Viewport, ww: number): number {
  return ww / vp.tpp;
}

/** Convert world-space height to screen pixels */
export function worldHeightToScreen(vp: Viewport, wh: number): number {
  return wh / vp.vpp;
}

/**
 * Return true if the world-space rect [x, y, w, h] has any part
 * visible inside the current viewport.
 */
export function isRectVisible(vp: Viewport, x: number, y: number, w: number, h: number): boolean {
  const rightEdge = vp.left + vp.width * vp.tpp;
  const bottomEdge = vp.top + vp.height * vp.vpp;
  return x + w > vp.left && x < rightEdge && y + h > vp.top && y < bottomEdge;
}

/** Pan the viewport by a screen-space pixel delta */
export function pan(vp: Viewport, delta: PanDelta): Viewport {
  return {
    ...vp,
    left: vp.left - delta.dx * vp.tpp,
    top: vp.top - delta.dy * vp.vpp,
  };
}

/**
 * Zoom around a screen-space anchor point. The world point under the anchor
 * stays fixed.
 */
export function zoom(vp: Viewport, anchor: ZoomAnchor): Viewport {
  const { screenX, screenY, factor } = anchor;
  const worldAnchorX = screenXToWorld(vp, screenX);
  const worldAnchorY = screenYToWorld(vp, screenY);
  const newTpp = vp.tpp / factor;
  const newVpp = vp.vpp / factor;
  return {
    ...vp,
    tpp: newTpp,
    vpp: newVpp,
    left: worldAnchorX - screenX * newTpp,
    top: worldAnchorY - screenY * newVpp,
  };
}

/** Zoom only the horizontal axis around a screen-space x pivot */
export function zoomX(vp: Viewport, screenX: number, factor: number): Viewport {
  const worldAnchorX = screenXToWorld(vp, screenX);
  const newTpp = vp.tpp / factor;
  return {
    ...vp,
    tpp: newTpp,
    left: worldAnchorX - screenX * newTpp,
  };
}

/** Clamp the viewport so it never scrolls past [minLeft, maxRight] in x */
export function clampX(vp: Viewport, minLeft: number, maxRight: number): Viewport {
  const vpWidthWorld = vp.width * vp.tpp;
  let left = vp.left;
  if (left < minLeft) left = minLeft;
  if (left + vpWidthWorld > maxRight) left = maxRight - vpWidthWorld;
  return { ...vp, left };
}

/** Build an initial viewport that fits [worldLeft, worldRight] horizontally */
export function fitX(
  width: number,
  height: number,
  worldLeft: number,
  worldRight: number,
  vpp = 1,
): Viewport {
  const span = worldRight - worldLeft;
  return {
    tpp: span / width,
    vpp,
    left: worldLeft,
    top: 0,
    width,
    height,
  };
}
