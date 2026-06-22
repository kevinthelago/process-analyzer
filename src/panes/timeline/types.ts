/** A single timed event from the Arrow IPC table */
export interface TimelineEvent {
  /** Start time in nanoseconds */
  timeNs: number;
  /** Duration in nanoseconds */
  durationNs: number;
  pid: number;
  tid: number;
  name: string;
  /** 'cpu' | 'io' | 'syscall' | 'user' | ... */
  kind: string;
  /** Nesting depth (0 = top-level) */
  depth: number;
}

/** One rendered track (one thread or process) */
export interface Track {
  label: string;
  pid: number;
  tid: number | null;
  events: TimelineEvent[];
  /** Track y-offset in world pixels from scene top */
  yOffset: number;
  height: number;
}

export const TRACK_HEIGHT = 24;
export const TRACK_GAP = 2;
export const RULER_HEIGHT = 28;

/** Color palette by event kind */
export const KIND_COLOR: Record<string, number> = {
  cpu: 0x4e9af1,
  io: 0xf4a53d,
  syscall: 0x9b59b6,
  user: 0x2ecc71,
  idle: 0x2c3e50,
};

export function kindColor(kind: string): number {
  return KIND_COLOR[kind] ?? 0x607d8b;
}
