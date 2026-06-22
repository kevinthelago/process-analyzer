/** A diff node pairs baseline and regression call trees */
export interface DiffNode {
  id: number;
  parentId: number;
  frame: string;
  baselineNs: number;
  regressionNs: number;
  /** (regression - baseline) / baseline * 100 */
  deltaPct: number;
  depth: number;
  children: DiffNode[];
}

/** Laid-out rect for a diff frame */
export interface DiffRect {
  nodeId: number;
  frame: string;
  baselineNs: number;
  regressionNs: number;
  deltaPct: number;
  x: number;
  y: number;
  w: number;
  h: number;
  color: number;
}

/** A row in the tabular summary (top regressions / improvements) */
export interface DiffTableRow {
  frame: string;
  baselineNs: number;
  regressionNs: number;
  deltaPct: number;
  deltaAbsNs: number;
}

/** Delta threshold below which a frame is considered unchanged (%) */
export const DELTA_THRESHOLD = 5;

/** Colors */
export const COLOR_REGRESSION = 0xe05252;
export const COLOR_IMPROVEMENT = 0x3fb950;
export const COLOR_UNCHANGED = 0x3d444d;

export function diffColor(deltaPct: number): number {
  if (deltaPct > DELTA_THRESHOLD) return COLOR_REGRESSION;
  if (deltaPct < -DELTA_THRESHOLD) return COLOR_IMPROVEMENT;
  return COLOR_UNCHANGED;
}
