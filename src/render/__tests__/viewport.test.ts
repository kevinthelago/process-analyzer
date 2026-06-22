import { describe, expect, it } from 'vitest';
import {
  clampX,
  fitX,
  isRectVisible,
  pan,
  screenXToWorld,
  worldXToScreen,
  zoom,
  zoomX,
} from '../viewport';
import type { Viewport } from '../types';

const BASE: Viewport = {
  tpp: 1,
  vpp: 1,
  left: 0,
  top: 0,
  width: 800,
  height: 600,
};

describe('coordinate transforms', () => {
  it('screenXToWorld at origin', () => {
    expect(screenXToWorld(BASE, 0)).toBe(0);
  });

  it('screenXToWorld at right edge', () => {
    expect(screenXToWorld(BASE, 800)).toBe(800);
  });

  it('worldXToScreen round-trips', () => {
    const world = 123.456;
    expect(worldXToScreen(BASE, screenXToWorld(BASE, world))).toBeCloseTo(world);
  });

  it('screenXToWorld with nonzero left', () => {
    const vp: Viewport = { ...BASE, left: 100 };
    expect(screenXToWorld(vp, 0)).toBe(100);
  });

  it('screenXToWorld with tpp > 1 (zoomed out)', () => {
    const vp: Viewport = { ...BASE, tpp: 2 };
    expect(screenXToWorld(vp, 100)).toBe(200);
  });
});

describe('isRectVisible', () => {
  it('fully inside', () => {
    expect(isRectVisible(BASE, 10, 10, 100, 100)).toBe(true);
  });

  it('partially inside (left edge)', () => {
    expect(isRectVisible(BASE, -50, 10, 100, 100)).toBe(true);
  });

  it('completely to the left', () => {
    expect(isRectVisible(BASE, -200, 10, 100, 100)).toBe(false);
  });

  it('completely to the right', () => {
    expect(isRectVisible(BASE, 900, 10, 100, 100)).toBe(false);
  });

  it('touching right edge is invisible', () => {
    // rect ends exactly at left edge of viewport
    expect(isRectVisible(BASE, -100, 10, 100, 10)).toBe(false);
  });
});

describe('pan', () => {
  it('panning right moves left up', () => {
    const vp = pan(BASE, { dx: 100, dy: 0 });
    expect(vp.left).toBeCloseTo(-100);
  });

  it('does not mutate original', () => {
    pan(BASE, { dx: 50, dy: 50 });
    expect(BASE.left).toBe(0);
  });
});

describe('zoom', () => {
  it('zoom in halves tpp', () => {
    const vp = zoom(BASE, { screenX: 400, screenY: 300, factor: 2 });
    expect(vp.tpp).toBeCloseTo(0.5);
  });

  it('anchor point stays fixed under zoom', () => {
    const anchorScreen = 400;
    const anchorWorld = screenXToWorld(BASE, anchorScreen);
    const vp = zoom(BASE, { screenX: anchorScreen, screenY: 300, factor: 2 });
    const anchorWorldAfter = screenXToWorld(vp, anchorScreen);
    expect(anchorWorldAfter).toBeCloseTo(anchorWorld);
  });
});

describe('zoomX', () => {
  it('zooms only horizontal axis', () => {
    const vp = zoomX(BASE, 400, 2);
    expect(vp.tpp).toBeCloseTo(0.5);
    expect(vp.vpp).toBe(BASE.vpp);
  });
});

describe('clampX', () => {
  it('clamps left boundary', () => {
    const vp: Viewport = { ...BASE, left: -100 };
    expect(clampX(vp, 0, 2000).left).toBe(0);
  });

  it('clamps right boundary', () => {
    const vp: Viewport = { ...BASE, left: 1800, width: 800, tpp: 1 };
    expect(clampX(vp, 0, 2000).left).toBe(1200);
  });
});

describe('fitX', () => {
  it('fits world range into viewport', () => {
    const vp = fitX(800, 600, 0, 1600);
    expect(vp.tpp).toBe(2);
    expect(vp.left).toBe(0);
  });
});
