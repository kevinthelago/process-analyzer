import React, { useCallback, useState } from 'react';
import type { PresetConfig } from '@/contracts/query-engine';
import { PRESETS } from './presetDefinitions';
import { cn } from '@/lib/utils';

/**
 * PresetState manages the "active preset" concept.
 * Applying a preset stores the previous state so revert can restore it.
 * The actual table sort/filter is driven via a Zustand preset store that
 * ProcessTable and ThreadTable subscribe to.
 *
 * Contract: the app-shell or query-analysis stream owns usePresetStore.
 * We emit changes here; consumers (ProcessTable, ThreadTable) apply them.
 */
import { usePresetStore } from '@/store/preset';

interface PresetButtonProps {
  preset: PresetConfig;
  isActive: boolean;
  onApply: (preset: PresetConfig) => void;
  onRevert: () => void;
}

function PresetButton({ preset, isActive, onApply, onRevert }: PresetButtonProps) {
  return (
    <div
      className={cn(
        'flex items-center justify-between gap-2 rounded px-2 py-1.5 text-sm transition-colors',
        isActive ? 'bg-primary/10 ring-1 ring-primary/30' : 'hover:bg-muted/60',
      )}
    >
      <div className="min-w-0">
        <div className="font-medium text-sm">{preset.label}</div>
        <div className="text-xs text-muted-foreground truncate">{preset.description}</div>
      </div>
      <div className="flex gap-1 shrink-0">
        {isActive ? (
          <button
            className="text-xs text-muted-foreground hover:text-foreground border border-border/60 rounded px-1.5 py-0.5 transition-colors"
            onClick={onRevert}
          >
            Revert
          </button>
        ) : (
          <button
            className="text-xs text-primary hover:text-primary/80 border border-primary/30 rounded px-1.5 py-0.5 transition-colors"
            onClick={() => onApply(preset)}
          >
            Apply
          </button>
        )}
      </div>
    </div>
  );
}

export function Presets() {
  const activePresetId = usePresetStore(s => s.activePresetId);
  const applyPreset = usePresetStore(s => s.applyPreset);
  const revertPreset = usePresetStore(s => s.revertPreset);

  return (
    <div className="px-2 pb-3 space-y-1">
      {PRESETS.map(preset => (
        <PresetButton
          key={preset.id}
          preset={preset}
          isActive={activePresetId === preset.id}
          onApply={applyPreset}
          onRevert={revertPreset}
        />
      ))}
    </div>
  );
}
