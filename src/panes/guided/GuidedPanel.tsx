import React from 'react';
import type { Finding, FindingSeverity } from '@/contracts/query-engine';
import { useSelectionStore } from '@/store/selection';
import { useFindings } from './useFindings';
import { Presets } from './Presets';
import { cn } from '@/lib/utils';

// ─── Severity badge ───────────────────────────────────────────────────────────

const SEVERITY_STYLES: Record<FindingSeverity, string> = {
  critical: 'bg-red-600 text-white',
  high: 'bg-orange-500 text-white',
  medium: 'bg-amber-400 text-black',
  low: 'bg-sky-500 text-white',
  info: 'bg-muted text-muted-foreground',
};

function SeverityBadge({ severity }: { severity: FindingSeverity }) {
  return (
    <span
      className={cn(
        'inline-block px-1.5 py-0.5 rounded text-[10px] font-bold uppercase tracking-wide',
        SEVERITY_STYLES[severity],
      )}
    >
      {severity}
    </span>
  );
}

// ─── Finding card ─────────────────────────────────────────────────────────────

interface FindingCardProps {
  finding: Finding;
  onDismiss: (id: string) => void;
}

function FindingCard({ finding, onDismiss }: FindingCardProps) {
  const jumpTo = useSelectionStore(s => s.jumpTo);

  const handleJump = () => {
    jumpTo({
      pid: finding.pid ?? null,
      tid: finding.tid ?? null,
    });
  };

  return (
    <div
      className={cn(
        'rounded-lg border p-3 mb-2 transition-colors group',
        finding.severity === 'critical' && 'border-red-600/40 bg-red-600/5',
        finding.severity === 'high' && 'border-orange-500/40 bg-orange-500/5',
        finding.severity === 'medium' && 'border-amber-400/30 bg-amber-400/5',
        finding.severity === 'low' && 'border-sky-500/30 bg-sky-500/5',
        finding.severity === 'info' && 'border-border bg-muted/20',
      )}
    >
      <div className="flex items-start justify-between gap-2 mb-1.5">
        <div className="flex items-center gap-2 min-w-0">
          <SeverityBadge severity={finding.severity} />
          <span className="text-xs text-muted-foreground">{finding.category}</span>
        </div>
        <button
          className="opacity-0 group-hover:opacity-60 hover:opacity-100 text-muted-foreground hover:text-foreground text-xs transition-opacity shrink-0"
          onClick={() => onDismiss(finding.id)}
          aria-label="Dismiss"
        >
          ✕
        </button>
      </div>

      <p className="text-sm font-medium mb-1 leading-snug">{finding.title}</p>
      <p className="text-xs text-muted-foreground leading-relaxed">{finding.description}</p>

      {(finding.pid != null || finding.tid != null) && (
        <button
          className="mt-2 text-xs text-primary hover:underline"
          onClick={handleJump}
        >
          Jump to {finding.tid != null ? `thread ${finding.tid}` : `process ${finding.pid}`} →
        </button>
      )}
    </div>
  );
}

// ─── Main panel ───────────────────────────────────────────────────────────────

export function GuidedPanel() {
  const { findings, dismiss, clearAll } = useFindings();

  return (
    <div className="flex flex-col h-full">
      {/* Presets section */}
      <div className="shrink-0 border-b">
        <div className="px-3 py-2 text-xs font-semibold uppercase tracking-wider text-muted-foreground">
          Investigation Presets
        </div>
        <Presets />
      </div>

      {/* Findings section */}
      <div className="shrink-0 border-b px-3 py-2 flex items-center justify-between">
        <span className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">
          Findings
          {findings.length > 0 && (
            <span className="ml-1.5 inline-block bg-primary text-primary-foreground text-[10px] rounded-full px-1.5 py-0.5">
              {findings.length}
            </span>
          )}
        </span>
        {findings.length > 0 && (
          <button
            className="text-xs text-muted-foreground hover:text-foreground transition-colors"
            onClick={clearAll}
          >
            Clear all
          </button>
        )}
      </div>

      <div className="flex-1 overflow-auto px-3 py-2">
        {findings.length === 0 ? (
          <div className="flex flex-col items-center justify-center h-32 text-muted-foreground text-sm text-center gap-2">
            <span className="text-xl opacity-30">✓</span>
            <p>No findings yet</p>
            <p className="text-xs">Start a capture to see analysis results</p>
          </div>
        ) : (
          findings.map(f => (
            <FindingCard key={f.id} finding={f} onDismiss={dismiss} />
          ))
        )}
      </div>
    </div>
  );
}
