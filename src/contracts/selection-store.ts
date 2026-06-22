/**
 * Selection store contract — owned by app-shell stream.
 * This file defines the interface that all pane consumers depend on.
 * The app-shell Zustand store must satisfy this interface at integration time.
 */

export interface TimeRange {
  startNs: number;
  endNs: number;
}

export interface StackFrame {
  address: number;
  symbol?: string;
  file?: string;
  line?: number;
  column?: number;
}

/** The full selection state shared across all panes. */
export interface SelectionState {
  // Current selection
  selectedPid: number | null;
  selectedTid: number | null;
  selectedFrame: StackFrame | null;
  timeRange: TimeRange | null;

  // Mutators — app-shell implements these
  setSelectedPid: (pid: number | null) => void;
  setSelectedTid: (tid: number | null) => void;
  setSelectedFrame: (frame: StackFrame | null) => void;
  setTimeRange: (range: TimeRange | null) => void;

  /** Jump the whole app to a specific location — used by guided panel. */
  jumpTo: (opts: {
    pid?: number | null;
    tid?: number | null;
    frame?: StackFrame | null;
    timeRange?: TimeRange | null;
  }) => void;
}

/**
 * Hook provided by app-shell via Zustand.
 * Import path at integration: @/store/selection
 * Re-exported here for type checking during parallel development.
 */
export type UseSelectionStore = {
  (): SelectionState;
  <T>(selector: (state: SelectionState) => T): T;
};
