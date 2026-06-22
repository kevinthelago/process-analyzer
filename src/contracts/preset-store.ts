/**
 * Preset store contract — shared between guided panel (writer) and
 * tables (readers). Owned by the app-shell stream.
 */
import type { PresetConfig, ProcessRow, ThreadRow } from './query-engine';

export interface PresetState {
  /** The currently active preset id, or null if none applied */
  activePresetId: string | null;
  /** The active preset config, or null */
  activePreset: PresetConfig | null;

  applyPreset: (preset: PresetConfig) => void;
  revertPreset: () => void;
}

export type UsePresetStore = {
  (): PresetState;
  <T>(selector: (state: PresetState) => T): T;
};
