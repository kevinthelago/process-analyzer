import { scaleLinear } from 'd3-scale';
import type { Viewport } from './types';

/**
 * Returns a d3 linear scale mapping world-space time units to screen pixels,
 * suitable for passing to d3-axis for tick generation.
 */
export function makeTimeScale(vp: Viewport) {
  return scaleLinear()
    .domain([vp.left, vp.left + vp.width * vp.tpp])
    .range([0, vp.width]);
}

/**
 * Returns a d3 linear scale mapping world-space value units to screen pixels.
 */
export function makeValueScale(vp: Viewport) {
  return scaleLinear()
    .domain([vp.top, vp.top + vp.height * vp.vpp])
    .range([0, vp.height]);
}

/** Human-readable nanosecond → string (µs, ms, s) */
export function formatNs(ns: number): string {
  if (ns < 1_000) return `${ns}ns`;
  if (ns < 1_000_000) return `${(ns / 1_000).toFixed(1)}µs`;
  if (ns < 1_000_000_000) return `${(ns / 1_000_000).toFixed(2)}ms`;
  return `${(ns / 1_000_000_000).toFixed(3)}s`;
}

/** Format percentage delta for diff display */
export function formatDeltaPct(pct: number): string {
  const sign = pct > 0 ? '+' : '';
  return `${sign}${pct.toFixed(1)}%`;
}
