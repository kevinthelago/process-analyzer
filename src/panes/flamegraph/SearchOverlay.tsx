import React, { useCallback, useEffect, useRef } from 'react';

interface SearchOverlayProps {
  value: string;
  matchCount: number;
  onChange: (q: string) => void;
  onClose: () => void;
}

/**
 * Floating search bar rendered over the flame graph canvas.
 * Keyboard shortcut: Ctrl+F / Cmd+F opens it.
 */
export const SearchOverlay: React.FC<SearchOverlayProps> = ({ value, matchCount, onChange, onClose }) => {
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLInputElement>) => {
      if (e.key === 'Escape') {
        onChange('');
        onClose();
      }
    },
    [onChange, onClose],
  );

  return (
    <div className="absolute top-2 right-2 z-20 flex items-center gap-2 rounded-md bg-[#161b22] border border-[#30363d] px-3 py-1.5 shadow-lg">
      <input
        ref={inputRef}
        type="text"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={handleKeyDown}
        placeholder="Search frames…"
        className="bg-transparent text-[#c9d1d9] placeholder-[#6e7681] text-sm outline-none w-48"
      />
      {value.length > 0 && (
        <span className="text-xs text-[#8b949e] whitespace-nowrap">
          {matchCount} match{matchCount !== 1 ? 'es' : ''}
        </span>
      )}
      <button
        onClick={() => { onChange(''); onClose(); }}
        className="text-[#6e7681] hover:text-[#c9d1d9] text-sm leading-none"
        aria-label="Close search"
      >
        ×
      </button>
    </div>
  );
};
